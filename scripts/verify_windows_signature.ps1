#Requires -Version 5.1
<#
.SYNOPSIS
Windows バイナリの Authenticode 署名（構造・信頼チェーン・SHA-256・タイムスタンプ）を検証する。

.DESCRIPTION
対象ファイル群に対して signtool.exe verify /pa /v および Get-AuthenticodeSignature を実行し、
署名状態、サムプリント一致、SHA-256 ダイジェスト、RFC 3161 タイムスタンプを検証する。
CI ランナー上でのみ -TrustCertPath を指定することで、秘密鍵を含まない公開証明書（.cer）を
Cert:\LocalMachine\Root へ一時登録して検証し、finally で必ず削除する。
-AuditDir を指定した場合は sign_windows.ps1 の監査ログ（失敗ゼロ・未完了ゼロ・対象一致・
NSIS アンインストーラのキャプチャ存在）を検証し、キャプチャ済みアンインストーラも署名検証対象へ加える。
#>
param(
    [Parameter(Mandatory = $true)]
    [string[]]$TargetPaths,

    [string]$ExpectedThumbprint = '',

    [string]$TrustCertPath = '',

    [string]$AuditDir = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($null -eq $TargetPaths -or $TargetPaths.Count -eq 0) {
    throw "検証対象ファイルが指定されていません。"
}

$resolvedTargets = @()
foreach ($p in $TargetPaths) {
    if ([string]::IsNullOrWhiteSpace($p) -or -not (Test-Path -LiteralPath $p -PathType Leaf)) {
        throw "検証対象ファイルが見つかりません: $p"
    }
    $resolvedTargets += (Resolve-Path -LiteralPath $p).ProviderPath
}

if (-not [string]::IsNullOrWhiteSpace($AuditDir)) {
    if (-not (Test-Path -LiteralPath $AuditDir -PathType Container)) {
        throw "署名監査ディレクトリが見つかりません: $AuditDir"
    }

    $utf8NoBom = [System.Text.UTF8Encoding]::new($false)
    $failFiles = @(Get-ChildItem -LiteralPath $AuditDir -Filter 'fail-*.json' -File -ErrorAction SilentlyContinue)
    if ($failFiles.Count -gt 0) {
        $failSummaries = @()
        foreach ($ff in $failFiles) {
            $failObj = [System.IO.File]::ReadAllText($ff.FullName, $utf8NoBom) | ConvertFrom-Json
            $failSummaries += "$($failObj.leaf) ($($failObj.error))"
        }
        throw "署名監査ログに署名失敗が記録されています ($($failFiles.Count) 件): $($failSummaries -join '; ')"
    }

    $startFiles = @(Get-ChildItem -LiteralPath $AuditDir -Filter 'start-*.json' -File -ErrorAction SilentlyContinue)
    if ($startFiles.Count -eq 0) {
        throw "署名監査ログに署名実行記録 (start-*.json) が存在しません: $AuditDir"
    }

    $okFiles = @(Get-ChildItem -LiteralPath $AuditDir -Filter 'ok-*.json' -File -ErrorAction SilentlyContinue)
    $okById = @{}
    $okTargets = New-Object 'System.Collections.Generic.HashSet[string]' ([System.StringComparer]::OrdinalIgnoreCase)
    $capturedPaths = @()
    foreach ($of in $okFiles) {
        $okObj = [System.IO.File]::ReadAllText($of.FullName, $utf8NoBom) | ConvertFrom-Json
        $okById[$okObj.id] = $okObj
        if (-not [string]::IsNullOrWhiteSpace($okObj.target)) {
            [void]$okTargets.Add($okObj.target)
        }
        if ($null -ne $okObj.PSObject.Properties['capturedPath'] -and -not [string]::IsNullOrWhiteSpace($okObj.capturedPath)) {
            $capturedPaths += $okObj.capturedPath
        }
    }

    foreach ($sf in $startFiles) {
        $startObj = [System.IO.File]::ReadAllText($sf.FullName, $utf8NoBom) | ConvertFrom-Json
        if (-not $okById.ContainsKey($startObj.id)) {
            throw "署名監査ログに未完了の署名実行があります (id=$($startObj.id), target=$($startObj.target))。"
        }
    }

    foreach ($target in $resolvedTargets) {
        if (-not $okTargets.Contains($target)) {
            throw "署名監査ログに検証対象ファイルの成功記録がありません: $target"
        }
    }

    if ($capturedPaths.Count -eq 0) {
        throw "署名監査ログに NSIS アンインストーラの署名キャプチャが見つかりません (!uninstfinalize の未実行または署名失敗の可能性があります)。"
    }

    foreach ($cap in $capturedPaths) {
        if (-not (Test-Path -LiteralPath $cap -PathType Leaf)) {
            throw "キャプチャ済みアンインストーラが見つかりません: $cap"
        }
        $resolvedCap = (Resolve-Path -LiteralPath $cap).ProviderPath
        if ($resolvedTargets -notcontains $resolvedCap) {
            $resolvedTargets += $resolvedCap
        }
    }
    Write-Host "署名監査ログ検証 OK (実行=$($startFiles.Count) 件、失敗=0 件、アンインストーラ捕捉=$($capturedPaths.Count) 件)"
}

$normalizedExpected = ''
if (-not [string]::IsNullOrWhiteSpace($ExpectedThumbprint)) {
    $normalizedExpected = ($ExpectedThumbprint -replace '\s+', '').ToUpperInvariant()
    if ($normalizedExpected -notmatch '^[0-9A-F]{40}$') {
        throw "ExpectedThumbprint の形式が不正です（40 桁の 16 進文字列が必要）。"
    }
}

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

$addedToRoot = $false
$trustThumbprint = ''
$verificationSucceeded = $false

try {
    if (-not [string]::IsNullOrWhiteSpace($TrustCertPath)) {
        if (-not (Test-Path -LiteralPath $TrustCertPath -PathType Leaf)) {
            throw "一時信頼登録用の証明書ファイルが見つかりません: $TrustCertPath"
        }
        $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
        if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            throw "-TrustCertPath の使用には管理者権限が必要です。"
        }

        $certObj = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2((Resolve-Path -LiteralPath $TrustCertPath).ProviderPath)
        try {
            if ($certObj.HasPrivateKey) {
                throw "-TrustCertPath に秘密鍵を含む証明書は指定できません。"
            }
            $trustThumbprint = $certObj.Thumbprint.ToUpperInvariant()
        } finally {
            $certObj.Dispose()
        }

        $rootStorePath = "Cert:\LocalMachine\Root\$trustThumbprint"
        if (-not (Test-Path -LiteralPath $rootStorePath)) {
            Import-Certificate -FilePath $TrustCertPath -CertStoreLocation 'Cert:\LocalMachine\Root' | Out-Null
            $addedToRoot = $true
            Write-Host "CI 検証用に公開証明書を一時登録しました: $trustThumbprint"
        } else {
            Write-Host "公開証明書は既に Cert:\LocalMachine\Root に登録済みです: $trustThumbprint"
        }
    }

    foreach ($target in $resolvedTargets) {
        Write-Host "署名検証中: $target"
        $prevPref = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $verifyOutputLines = @(& $signTool verify /pa /v $target 2>&1)
            $verifyExit = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $prevPref
        }
        $verifyText = ($verifyOutputLines | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine
        if (-not [string]::IsNullOrEmpty($verifyText)) {
            Write-Host $verifyText
        }
        if ($verifyExit -ne 0) {
            throw "signtool verify /pa /v が終了コード $verifyExit で失敗しました: $target"
        }
        if ($verifyText -notmatch '(?im)^[ \t]*Hash of file \(sha256\):[ \t]*[0-9a-f]{64}[ \t]*\r?$') {
            throw "signtool verify の出力に SHA-256 ファイルハッシュが確認できません: $target"
        }

        $sig = Get-AuthenticodeSignature -FilePath $target
        if ($sig.Status -ne [System.Management.Automation.SignatureStatus]::Valid) {
            throw "Get-AuthenticodeSignature が Valid ではありません ($($sig.Status): $($sig.StatusMessage)): $target"
        }
        if ($null -eq $sig.SignerCertificate) {
            throw "SignerCertificate が取得できません: $target"
        }
        $actualThumb = $sig.SignerCertificate.Thumbprint.ToUpperInvariant()
        if (-not [string]::IsNullOrEmpty($normalizedExpected) -and $actualThumb -ne $normalizedExpected) {
            throw "署名サムプリントが期待値と一致しません ($actualThumb != $normalizedExpected): $target"
        }
        if ($null -eq $sig.TimeStamperCertificate) {
            throw "RFC 3161 タイムスタンプ証明書が付与されていません: $target"
        }

        Write-Host "検証成功: $(Split-Path -Leaf $target) (Thumbprint=$actualThumb, TimeStamper=$($sig.TimeStamperCertificate.Subject))"
    }
    $verificationSucceeded = $true
} finally {
    if ($addedToRoot -and -not [string]::IsNullOrEmpty($trustThumbprint)) {
        try {
            $rootStorePath = "Cert:\LocalMachine\Root\$trustThumbprint"
            if (Test-Path -LiteralPath $rootStorePath) {
                Remove-Item -LiteralPath $rootStorePath -Force
                Write-Host "CI 検証用の一時ルート証明書を削除しました: $trustThumbprint"
            }
        } catch {
            Write-Warning "一時ルート証明書の削除に失敗しました ($trustThumbprint): $($_.Exception.Message)"
            if ($verificationSucceeded) {
                throw
            }
        }
    }
}

$global:LASTEXITCODE = 0
