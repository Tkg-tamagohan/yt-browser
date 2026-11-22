#Requires -Version 5.1
<#
.SYNOPSIS
Tauri の bundle.windows.signCommand から呼び出される Windows Authenticode 署名スクリプト。

.DESCRIPTION
環境変数 CODE_SIGN_CERT_THUMBPRINT に指定された Cert:\CurrentUser\My 上の証明書を使い、
Windows SDK の signtool.exe で SHA-256 署名と RFC 3161 タイムスタンプを付与する。
秘密鍵ファイル（.pfx）やパスワードを引数・環境変数へ残さない前提で動作する。
#>
param(
    [Parameter(Mandatory = $true)]
    [string]$TargetFile
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($TargetFile) -or -not (Test-Path -LiteralPath $TargetFile -PathType Leaf)) {
    throw "署名対象ファイルが見つかりません: $TargetFile"
}

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

$maxAttempts = 3
$retryDelaysSec = @(2, 5)
$signed = $false
$targetName = Split-Path -Leaf $TargetFile

for ($attempt = 1; $attempt -le $maxAttempts; $attempt++) {
    Write-Host "Authenticode 署名を実行中 ($attempt/$maxAttempts): $targetName"
    $prevPref = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $signTool sign /s My /sha1 $thumbprint /fd SHA256 /tr $timestampUrl /td SHA256 $TargetFile
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
