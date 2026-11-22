#Requires -Version 5.1
<#
.SYNOPSIS
Tauri の bundle.windows.signCommand から呼び出される Windows Authenticode 署名スクリプト。

.DESCRIPTION
環境変数 CODE_SIGN_CERT_THUMBPRINT に指定された Cert:\CurrentUser\My 上の証明書を使い、
Windows SDK の signtool.exe で SHA-256 署名と RFC 3161 タイムスタンプを付与する。
秘密鍵ファイル（.pfx）やパスワードを引数・環境変数へ残さない前提で動作する。
環境変数 CODE_SIGN_AUDIT_DIR が設定されている場合は開始・成功・失敗の監査 JSON を記録し、
CODE_SIGN_CAPTURE_DIR が設定されている場合は NSIS 一時アンインストーラの署名済みコピーを保全する。
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$TargetFile,

    [string]$SignToolPath = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$auditDir = if ([string]::IsNullOrWhiteSpace($env:CODE_SIGN_AUDIT_DIR)) { '' } else { $env:CODE_SIGN_AUDIT_DIR.Trim() }
$captureDir = if ([string]::IsNullOrWhiteSpace($env:CODE_SIGN_CAPTURE_DIR)) { '' } else { $env:CODE_SIGN_CAPTURE_DIR.Trim() }
$auditEnabled = (-not [string]::IsNullOrEmpty($auditDir)) -and (Test-Path -LiteralPath $auditDir -PathType Container)
$invocationId = [guid]::NewGuid().ToString('N')
$rawLeaf = if ([string]::IsNullOrWhiteSpace($TargetFile)) { '' } else { Split-Path -Leaf $TargetFile }

if ($auditEnabled) {
    $startRecord = [ordered]@{
        id        = $invocationId
        target    = $TargetFile
        leaf      = $rawLeaf
        startedAt = [DateTimeOffset]::UtcNow.ToString('o')
    }
    $startPath = Join-Path $auditDir ("start-$invocationId.json")
    [System.IO.File]::WriteAllText($startPath, ($startRecord | ConvertTo-Json -Depth 4), $utf8NoBom)
}

try {
    if ([string]::IsNullOrWhiteSpace($TargetFile) -or -not (Test-Path -LiteralPath $TargetFile -PathType Leaf)) {
        throw "署名対象ファイルが見つかりません: $TargetFile"
    }
    $resolvedTarget = (Resolve-Path -LiteralPath $TargetFile).ProviderPath
    $targetName = Split-Path -Leaf $resolvedTarget
    $targetExt = [System.IO.Path]::GetExtension($resolvedTarget)

    $rawThumbprint = $env:CODE_SIGN_CERT_THUMBPRINT
    if ([string]::IsNullOrWhiteSpace($rawThumbprint)) {
        throw "環境変数 CODE_SIGN_CERT_THUMBPRINT が未設定です。"
    }
    $thumbprint = ($rawThumbprint -replace '\s+', '').ToUpperInvariant()
    if ($thumbprint -notmatch '^[0-9A-F]{40}$') {
        throw "環境変数 CODE_SIGN_CERT_THUMBPRINT の形式が不正です（40 桁の 16 進文字列が必要）。"
    }

    $timestampUrl = if ([string]::IsNullOrWhiteSpace($env:CODE_SIGN_TIMESTAMP_URL)) {
        'http://timestamp.digicert.com'
    } else {
        $env:CODE_SIGN_TIMESTAMP_URL.Trim()
    }

    if (-not [string]::IsNullOrWhiteSpace($SignToolPath)) {
        if (-not (Test-Path -LiteralPath $SignToolPath -PathType Leaf)) {
            throw "指定された SignToolPath が見つかりません: $SignToolPath"
        }
        $signTool = (Resolve-Path -LiteralPath $SignToolPath).ProviderPath
    } else {
        $sdkRoot = 'C:\Program Files (x86)\Windows Kits\10\bin'
        if (-not (Test-Path -LiteralPath $sdkRoot -PathType Container)) {
            throw "Windows SDK のインストールディレクトリが見つかりません: $sdkRoot"
        }

        $candidates = @(
            Get-ChildItem -Path $sdkRoot -Filter 'signtool.exe' -Recurse -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -like '*\x64\signtool.exe' }
        )
        if ($candidates.Count -eq 0) {
            throw "Windows SDK の x64 signtool.exe が見つかりません: $sdkRoot"
        }

        $signTool = (
            $candidates |
                Sort-Object -Descending -Property {
                    $verDir = Split-Path -Leaf (Split-Path -Parent (Split-Path -Parent $_.FullName))
                    $parsed = $null
                    if ([version]::TryParse($verDir, [ref]$parsed)) {
                        $parsed
                    } else {
                        [version]'0.0.0.0'
                    }
                } |
                Select-Object -First 1
        ).FullName
    }

    $maxAttempts = 3
    $retryDelaysSec = @(2, 5)
    $signed = $false

    for ($attempt = 1; $attempt -le $maxAttempts; $attempt++) {
        Write-Host "Authenticode 署名を実行中 ($attempt/$maxAttempts): $targetName"
        $prevPref = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            & $signTool sign /s My /sha1 $thumbprint /fd SHA256 /tr $timestampUrl /td SHA256 $resolvedTarget
            $signExit = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $prevPref
        }
        if ($signExit -eq 0) {
            $signed = $true
            break
        }

        if ($attempt -lt $maxAttempts) {
            $waitSec = $retryDelaysSec[$attempt - 1]
            Write-Warning "signtool sign が終了コード $signExit で失敗しました。${waitSec} 秒後に再試行します。"
            Start-Sleep -Seconds $waitSec
        }
    }

    if (-not $signed) {
        throw "Authenticode 署名が $maxAttempts 回連続で失敗しました: $targetName"
    }

    $capturedPath = ''
    $captureEnabled = (-not [string]::IsNullOrEmpty($captureDir)) -and (Test-Path -LiteralPath $captureDir -PathType Container)
    if ($captureEnabled) {
        # NSIS の !uninstfinalize が渡す一時アンインストーラは直後に削除されるため、
        # 本体・セットアップ・プラグイン DLL 以外の署名済みバイナリを検証用に保全する。
        $isTransientUninstaller = ($targetExt -ine '.dll') -and ($targetName -ine 'yt-browser.exe') -and ($targetName -inotlike '*-setup.exe')
        if ($isTransientUninstaller) {
            $captureLeaf = if ($targetExt -ieq '.exe') { "$invocationId-$targetName" } else { "$invocationId-$targetName.exe" }
            $capturedPath = Join-Path $captureDir $captureLeaf
            Copy-Item -LiteralPath $resolvedTarget -Destination $capturedPath -Force
        }
    }

    if ($auditEnabled) {
        $okRecord = [ordered]@{
            id           = $invocationId
            target       = $resolvedTarget
            leaf         = $targetName
            ext          = $targetExt
            capturedPath = $capturedPath
            completedAt  = [DateTimeOffset]::UtcNow.ToString('o')
        }
        $okPath = Join-Path $auditDir ("ok-$invocationId.json")
        [System.IO.File]::WriteAllText($okPath, ($okRecord | ConvertTo-Json -Depth 4), $utf8NoBom)
    }

    $global:LASTEXITCODE = 0
} catch {
    if ($auditEnabled) {
        try {
            $failRecord = [ordered]@{
                id       = $invocationId
                target   = $TargetFile
                leaf     = $rawLeaf
                error    = $_.Exception.Message
                failedAt = [DateTimeOffset]::UtcNow.ToString('o')
            }
            $failPath = Join-Path $auditDir ("fail-$invocationId.json")
            [System.IO.File]::WriteAllText($failPath, ($failRecord | ConvertTo-Json -Depth 4), $utf8NoBom)
        } catch {
            Write-Warning "署名失敗の監査記録書き込みに失敗しました: $($_.Exception.Message)"
        }
    }
    throw
}
