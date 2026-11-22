#Requires -Version 5.1
<#
.SYNOPSIS
release.yml の保険クリーンアップステップとして Windows 署名一時ファイルと証明書ストアを掃討する。

.DESCRIPTION
RUNNER_TEMP 配下の codesign-* 一時ディレクトリ（内部の .pfx はゼロ上書き後削除）および
環境変数 CODE_SIGN_CLEANUP_THUMBPRINTS / CODE_SIGN_CLEANUP_THUMBPRINT に記録された
全サムプリントを Cert:\CurrentUser\My と Cert:\LocalMachine\Root から削除し、残存ゼロを確認する。
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$tempRoot = if (-not [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    $env:RUNNER_TEMP
} else {
    [System.IO.Path]::GetTempPath()
}

$leftoverDirs = @(Get-ChildItem -Path $tempRoot -Filter 'codesign-*' -Directory -ErrorAction SilentlyContinue)
foreach ($dir in $leftoverDirs) {
    $pfxLeftovers = @(Get-ChildItem -Path $dir.FullName -Filter '*.pfx' -File -Recurse -ErrorAction SilentlyContinue)
    foreach ($f in $pfxLeftovers) {
        $len = $f.Length
        [System.IO.File]::WriteAllBytes($f.FullName, (New-Object byte[] $len))
        Remove-Item -LiteralPath $f.FullName -Force
    }
    Remove-Item -LiteralPath $dir.FullName -Recurse -Force
    Write-Host "保険クリーンアップで一時ディレクトリを削除しました: $($dir.FullName)"
}

$rawList = @()
if (-not [string]::IsNullOrWhiteSpace($env:CODE_SIGN_CLEANUP_THUMBPRINTS)) {
    $rawList += ($env:CODE_SIGN_CLEANUP_THUMBPRINTS -split ',')
}
if (-not [string]::IsNullOrWhiteSpace($env:CODE_SIGN_CLEANUP_THUMBPRINT)) {
    $rawList += ($env:CODE_SIGN_CLEANUP_THUMBPRINT -split ',')
}
$thumbprints = @(
    $rawList |
        ForEach-Object { ($_ -replace '\s+', '').ToUpperInvariant() } |
        Where-Object { -not [string]::IsNullOrWhiteSpace($_) } |
        Select-Object -Unique
)

foreach ($tp in $thumbprints) {
    $myPath = "Cert:\CurrentUser\My\$tp"
    if (Test-Path -LiteralPath $myPath) {
        Remove-Item -LiteralPath $myPath -Force
        if (Test-Path -LiteralPath $myPath) {
            & certutil.exe -user -delstore My $tp | Out-Null
        }
    }
    $rootPath = "Cert:\LocalMachine\Root\$tp"
    if (Test-Path -LiteralPath $rootPath) {
        Remove-Item -LiteralPath $rootPath -Force
    }

    if ((Test-Path -LiteralPath $myPath) -or (Test-Path -LiteralPath $rootPath)) {
        throw "証明書ストアに一時証明書が残存しています ($tp)。"
    }
}

$remainingDirs = @(Get-ChildItem -Path $tempRoot -Filter 'codesign-*' -Directory -ErrorAction SilentlyContinue)
if ($remainingDirs.Count -gt 0) {
    throw "一時領域に codesign-* が残存しています。"
}

Write-Host "Windows 署名一時ファイルの残存ゼロを確認しました。"
$global:LASTEXITCODE = 0
