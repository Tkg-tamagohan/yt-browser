#Requires -Version 5.1
<#
.SYNOPSIS
ci.yml の windows ジョブおよびローカル環境で Windows 署名補助スクリプト群を検証する。

.DESCRIPTION
Secrets・管理者権限なしで実行可能な PowerShell 5.1 構文・BOM 検査、正常系署名検証、
未署名 PE 拒否、NSIS アンインストーラ署名失敗・欠落検出、複数証明書 PFX 事前拒否を検証する。

テスト ID:
  WIN-SIGN-01: 全 Windows 署名 PowerShell スクリプトの UTF-8 BOM と PowerShell 5.1 構文検査
  WIN-SIGN-02: sign_windows.ps1 の CODE_SIGN_CERT_THUMBPRINT 未設定拒否
  WIN-SIGN-03: sign_windows.ps1 の CODE_SIGN_CERT_THUMBPRINT 形式不正拒否
  WIN-SIGN-04: 有効な未署名 Windows PE 実行ファイルに対する verify_windows_signature.ps1 の未署名拒否
  WIN-SIGN-05: 実 SHA-256 署名済みバイナリ（Windows SDK signtool.exe）に対する verify_windows_signature.ps1 正常検証
  WIN-SIGN-06: ExpectedThumbprint 不一致時の verify_windows_signature.ps1 拒否
  WIN-SIGN-07: NSIS アンインストーラ署名失敗が監査ログ経由で検出・拒否されること
  WIN-SIGN-08: NSIS アンインストーラ署名キャプチャ欠落・未署名キャプチャ拒否と正常通過
  WIN-SIGN-09: 複数証明書 PFX がストアへインポートされる前に拒否され残存ゼロであること
  WIN-SIGN-10: cleanup_windows_signing.ps1 が残存 codesign-* と一時 .pfx を掃討すること
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).ProviderPath

function Invoke-PsScript {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ScriptPath,
        [string[]]$ScriptArgs = @(),
        [hashtable]$EnvOverrides = @{}
    )

    $savedEnv = @{}
    foreach ($k in $EnvOverrides.Keys) {
        $existing = [Environment]::GetEnvironmentVariable($k, 'Process')
        $savedEnv[$k] = $existing
        $val = $EnvOverrides[$k]
        if ($null -eq $val) {
            [Environment]::SetEnvironmentVariable($k, $null, 'Process')
        } else {
            [Environment]::SetEnvironmentVariable($k, [string]$val, 'Process')
        }
    }

    $prevPref = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $cmdArgs = @('-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-File', $ScriptPath) + $ScriptArgs
        $lines = @(& powershell.exe @cmdArgs 2>&1)
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $prevPref
        foreach ($k in $savedEnv.Keys) {
            [Environment]::SetEnvironmentVariable($k, $savedEnv[$k], 'Process')
        }
    }

    return [pscustomobject]@{
        ExitCode = $exitCode
        Output   = ($lines | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine
    }
}

# WIN-SIGN-01: 全 Windows 署名 PowerShell スクリプトの UTF-8 BOM と PowerShell 5.1 構文検査
$psScripts = @(
    'scripts\sign_windows.ps1',
    'scripts\verify_windows_signature.ps1',
    'scripts\build_and_sign_windows.ps1',
    'scripts\cleanup_windows_signing.ps1',
    'scripts\test_windows_signing_ci.ps1'
)
foreach ($rel in $psScripts) {
    $full = Join-Path $repoRoot $rel
    if (-not (Test-Path -LiteralPath $full -PathType Leaf)) {
        throw "[WIN-SIGN-01] スクリプトが見つかりません: $rel"
    }
    $bytes = [System.IO.File]::ReadAllBytes($full)
    if ($bytes.Length -lt 3 -or $bytes[0] -ne 0xEF -or $bytes[1] -ne 0xBB -or $bytes[2] -ne 0xBF) {
        throw "[WIN-SIGN-01] UTF-8 BOM がありません (CP1252 環境での日本語誤読防止に必須): $rel"
    }
    $tokens = $null
    $errors = $null
    [System.Management.Automation.Language.Parser]::ParseFile($full, [ref]$tokens, [ref]$errors) | Out-Null
    if ($errors.Count -gt 0) {
        throw "[WIN-SIGN-01] PowerShell 5.1 構文エラー ($rel): $($errors -join '; ')"
    }
}
Write-Host "[WIN-SIGN-01] OK: 全 $($psScripts.Count) スクリプトの UTF-8 BOM と PowerShell 5.1 構文を確認"

# Windows SDK の最新 x64 signtool.exe を探索（WIN-SIGN-05/06/08 の実署名バイナリ検証に使用）
$sdkRoot = 'C:\Program Files (x86)\Windows Kits\10\bin'
$signToolCandidates = @(
    Get-ChildItem -Path $sdkRoot -Filter 'signtool.exe' -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -like '*\x64\signtool.exe' }
)
if ($signToolCandidates.Count -eq 0) {
    throw "Windows SDK の x64 signtool.exe が見つかりません: $sdkRoot"
}
$signToolPath = (
    $signToolCandidates |
        Sort-Object -Descending -Property {
            $verDir = Split-Path -Leaf (Split-Path -Parent (Split-Path -Parent $_.FullName))
            $parsed = $null
            if ([version]::TryParse($verDir, [ref]$parsed)) { $parsed } else { [version]'0.0.0.0' }
        } |
        Select-Object -First 1
).FullName

$tempRoot = if (-not [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    $env:RUNNER_TEMP
} else {
    [System.IO.Path]::GetTempPath()
}
$sandbox = Join-Path $tempRoot ("win-sign-test-" + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $sandbox -Force | Out-Null

$signScript = Join-Path $repoRoot 'scripts\sign_windows.ps1'
$verifyScript = Join-Path $repoRoot 'scripts\verify_windows_signature.ps1'
$buildScript = Join-Path $repoRoot 'scripts\build_and_sign_windows.ps1'

try {
    # 有効な未署名 Windows PE 実行ファイルを生成（単なる MZ バイト列ではなく本物の PE ヘッダを持つ）
    $unsignedPe = Join-Path $sandbox 'unsigned_valid_pe.exe'
    Add-Type -TypeDefinition 'public class YtBrowserUnsignedPeFixture { public static void Main() {} }' -OutputAssembly $unsignedPe -OutputType ConsoleApplication
    if (-not (Test-Path -LiteralPath $unsignedPe -PathType Leaf)) {
        throw "テスト用の未署名 PE 実行ファイルを生成できませんでした: $unsignedPe"
    }

    # WIN-SIGN-02: sign_windows.ps1 サムプリント未設定拒否
    $res02 = Invoke-PsScript -ScriptPath $signScript -ScriptArgs @('-TargetFile', $unsignedPe) -EnvOverrides @{
        CODE_SIGN_CERT_THUMBPRINT = $null
        CODE_SIGN_AUDIT_DIR       = $null
        CODE_SIGN_CAPTURE_DIR     = $null
    }
    if ($res02.ExitCode -eq 0) {
        throw "[WIN-SIGN-02] sign_windows.ps1 が CODE_SIGN_CERT_THUMBPRINT 未設定で成功しました。"
    }
    if ($res02.Output -notmatch 'CODE_SIGN_CERT_THUMBPRINT') {
        throw "[WIN-SIGN-02] 想定外のエラー理由です: $($res02.Output)"
    }
    Write-Host "[WIN-SIGN-02] OK: CODE_SIGN_CERT_THUMBPRINT 未設定を拒否 (exit=$($res02.ExitCode))"

    # WIN-SIGN-03: sign_windows.ps1 サムプリント形式不正拒否
    $res03 = Invoke-PsScript -ScriptPath $signScript -ScriptArgs @('-TargetFile', $unsignedPe) -EnvOverrides @{
        CODE_SIGN_CERT_THUMBPRINT = 'INVALID-THUMBPRINT'
        CODE_SIGN_AUDIT_DIR       = $null
        CODE_SIGN_CAPTURE_DIR     = $null
    }
    if ($res03.ExitCode -eq 0) {
        throw "[WIN-SIGN-03] sign_windows.ps1 が不正サムプリントで成功しました。"
    }
    if ($res03.Output -notmatch '40') {
        throw "[WIN-SIGN-03] 想定外のエラー理由です: $($res03.Output)"
    }
    Write-Host "[WIN-SIGN-03] OK: サムプリント形式不正を拒否 (exit=$($res03.ExitCode))"

    # WIN-SIGN-04: 有効な未署名 PE に対する verify_windows_signature.ps1 の未署名拒否
    $res04 = Invoke-PsScript -ScriptPath $verifyScript -ScriptArgs @('-TargetPaths', $unsignedPe)
    if ($res04.ExitCode -eq 0) {
        throw "[WIN-SIGN-04] verify_windows_signature.ps1 が未署名 PE 実行ファイルで成功しました。"
    }
    if ($res04.Output -notmatch 'No signature found') {
        throw "[WIN-SIGN-04] 未署名による拒否（No signature found）ではなく別エラーになりました: $($res04.Output)"
    }
    Write-Host "[WIN-SIGN-04] OK: 有効な未署名 PE 実行ファイルを 'No signature found' で拒否 (exit=$($res04.ExitCode))"

    # WIN-SIGN-05: 実 SHA-256 署名済みバイナリ（signtool.exe）の正常検証
    $res05 = Invoke-PsScript -ScriptPath $verifyScript -ScriptArgs @('-TargetPaths', $signToolPath)
    if ($res05.ExitCode -ne 0) {
        throw "[WIN-SIGN-05] 実 SHA-256 署名済みバイナリの検証に失敗しました (exit=$($res05.ExitCode)): $($res05.Output)"
    }
    Write-Host "[WIN-SIGN-05] OK: 実 SHA-256 署名済みバイナリ (signtool.exe) の正常検証を通過"

    # WIN-SIGN-06: ExpectedThumbprint 不一致時の拒否
    $res06 = Invoke-PsScript -ScriptPath $verifyScript -ScriptArgs @('-TargetPaths', $signToolPath, '-ExpectedThumbprint', ('0' * 40))
    if ($res06.ExitCode -eq 0) {
        throw "[WIN-SIGN-06] ExpectedThumbprint 不一致で verify_windows_signature.ps1 が成功しました。"
    }
    if ($res06.Output -notmatch '0000000000000000000000000000000000000000') {
        throw "[WIN-SIGN-06] サムプリント不一致メッセージが確認できません: $($res06.Output)"
    }
    Write-Host "[WIN-SIGN-06] OK: ExpectedThumbprint 不一致を拒否 (exit=$($res06.ExitCode))"

    # WIN-SIGN-07: アンインストーラ署名失敗が監査ログ経由で確実に拒否されること
    $auditFailDir = Join-Path $sandbox 'audit-uninst-fail'
    $captureFailDir = Join-Path $sandbox 'capture-uninst-fail'
    New-Item -ItemType Directory -Path $auditFailDir -Force | Out-Null
    New-Item -ItemType Directory -Path $captureFailDir -Force | Out-Null

    # 本体・セットアップ相当は成功記録あり、アンインストーラ相当だけ sign_windows.ps1 が失敗する状況を再現
    $okMainId = '11111111111111111111111111111111'
    [System.IO.File]::WriteAllText((Join-Path $auditFailDir "start-$okMainId.json"), (@{ id = $okMainId; target = $signToolPath; leaf = 'signtool.exe' } | ConvertTo-Json), $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $auditFailDir "ok-$okMainId.json"), (@{ id = $okMainId; target = $signToolPath; leaf = 'signtool.exe'; capturedPath = '' } | ConvertTo-Json), $utf8NoBom)

    $fakeUninst = Join-Path $sandbox 'uninst_temp.exe'
    Copy-Item -LiteralPath $unsignedPe -Destination $fakeUninst -Force
    $resUninstSign = Invoke-PsScript -ScriptPath $signScript -ScriptArgs @('-TargetFile', $fakeUninst) -EnvOverrides @{
        CODE_SIGN_CERT_THUMBPRINT = $null
        CODE_SIGN_AUDIT_DIR       = $auditFailDir
        CODE_SIGN_CAPTURE_DIR     = $captureFailDir
    }
    if ($resUninstSign.ExitCode -eq 0) {
        throw "[WIN-SIGN-07] アンインストーラ署名失敗の再現に失敗しました。"
    }
    $res07 = Invoke-PsScript -ScriptPath $verifyScript -ScriptArgs @('-TargetPaths', $signToolPath, '-AuditDir', $auditFailDir)
    if ($res07.ExitCode -eq 0) {
        throw "[WIN-SIGN-07] アンインストーラ署名失敗が記録されているのに verify_windows_signature.ps1 が成功しました。"
    }
    if ($res07.Output -notmatch 'uninst_temp\.exe') {
        throw "[WIN-SIGN-07] 失敗したアンインストーラ名がエラー出力に含まれません: $($res07.Output)"
    }
    Write-Host "[WIN-SIGN-07] OK: アンインストーラ署名失敗を監査ログ経由で拒否 (exit=$($res07.ExitCode))"

    # WIN-SIGN-08: アンインストーラ署名キャプチャ欠落時の拒否と、正常キャプチャ存在時の通過
    $auditMissingCapDir = Join-Path $sandbox 'audit-missing-cap'
    New-Item -ItemType Directory -Path $auditMissingCapDir -Force | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $auditMissingCapDir "start-$okMainId.json"), (@{ id = $okMainId; target = $signToolPath; leaf = 'signtool.exe' } | ConvertTo-Json), $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $auditMissingCapDir "ok-$okMainId.json"), (@{ id = $okMainId; target = $signToolPath; leaf = 'signtool.exe'; capturedPath = '' } | ConvertTo-Json), $utf8NoBom)

    $res08a = Invoke-PsScript -ScriptPath $verifyScript -ScriptArgs @('-TargetPaths', $signToolPath, '-AuditDir', $auditMissingCapDir)
    if ($res08a.ExitCode -eq 0) {
        throw "[WIN-SIGN-08] アンインストーラ署名キャプチャ欠落なのに verify_windows_signature.ps1 が成功しました。"
    }
    if ($res08a.Output -notmatch 'uninstfinalize') {
        throw "[WIN-SIGN-08] アンインストーラ欠落エラーの理由が想定外です: $($res08a.Output)"
    }

    $auditValidCapDir = Join-Path $sandbox 'audit-valid-cap'
    $captureValidDir = Join-Path $sandbox 'capture-valid-cap'
    New-Item -ItemType Directory -Path $auditValidCapDir -Force | Out-Null
    New-Item -ItemType Directory -Path $captureValidDir -Force | Out-Null

    # sign_windows.ps1 の実際の監査・一時アンインストーラ捕捉経路（本体・DLL・一時アンインストーラ・セットアップ）を通す
    $mockSignTool = Join-Path $sandbox 'mock_signtool.cmd'
    $mockCmdLines = @(
        '@echo off',
        'if /I not "%~1"=="sign" exit /b 91',
        'if /I not "%~2"=="/s" exit /b 92',
        'if /I not "%~3"=="My" exit /b 93',
        'if /I not "%~4"=="/sha1" exit /b 94',
        'if /I not "%~6"=="/fd" exit /b 95',
        'if /I not "%~7"=="SHA256" exit /b 96',
        'if /I not "%~8"=="/tr" exit /b 97',
        'shift',
        'shift',
        'shift',
        'shift',
        'shift',
        'shift',
        'shift',
        'shift',
        'shift',
        'if /I not "%~1"=="/td" exit /b 98',
        'if /I not "%~2"=="SHA256" exit /b 99',
        'if "%~3"=="" exit /b 100',
        'exit /b 0'
    )
    [System.IO.File]::WriteAllText($mockSignTool, (($mockCmdLines -join "`r`n") + "`r`n"), [System.Text.Encoding]::ASCII)

    $simApp = Join-Path $sandbox 'yt-browser.exe'
    $simDll = Join-Path $sandbox 'nsis_tauri_utils.dll'
    $simUninst = Join-Path $sandbox 'nsu1234.exe'
    $simSetup = Join-Path $sandbox 'yt-browser_0.11.11_x64-setup.exe'
    Copy-Item -LiteralPath $signToolPath -Destination $simApp -Force
    Copy-Item -LiteralPath $signToolPath -Destination $simDll -Force
    Copy-Item -LiteralPath $signToolPath -Destination $simUninst -Force
    Copy-Item -LiteralPath $signToolPath -Destination $simSetup -Force

    $signEnv = @{
        CODE_SIGN_CERT_THUMBPRINT = ('A' * 40)
        CODE_SIGN_AUDIT_DIR       = $auditValidCapDir
        CODE_SIGN_CAPTURE_DIR     = $captureValidDir
    }
    foreach ($simTarget in @($simApp, $simDll, $simUninst, $simSetup)) {
        $rSign = Invoke-PsScript -ScriptPath $signScript -ScriptArgs @('-TargetFile', $simTarget, '-SignToolPath', $mockSignTool) -EnvOverrides $signEnv
        if ($rSign.ExitCode -ne 0) {
            throw "[WIN-SIGN-08] sign_windows.ps1 の捕捉経路テストに失敗しました ($simTarget, exit=$($rSign.ExitCode)): $($rSign.Output)"
        }
    }
    # NSIS が !uninstfinalize 直後に一時アンインストーラを削除する挙動を再現する
    Remove-Item -LiteralPath $simUninst -Force

    $capturedFiles = @(Get-ChildItem -LiteralPath $captureValidDir -File -ErrorAction SilentlyContinue)
    if ($capturedFiles.Count -ne 1 -or $capturedFiles[0].Name -notlike '*-nsu1234.exe') {
        $names = ($capturedFiles | ForEach-Object { $_.Name }) -join ', '
        throw "[WIN-SIGN-08] 一時アンインストーラのみが 1 件捕捉されるべきですが実測は ($($capturedFiles.Count) 件: $names) です。"
    }

    # build_and_sign_windows.ps1 と同様に string[] 配列で複数対象を渡し、捕捉済みアンインストーラまで検証する
    $verify08Output = (& $verifyScript -TargetPaths @($simApp, $simSetup) -AuditDir $auditValidCapDir *>&1 | ForEach-Object { $_.ToString() }) -join [Environment]::NewLine
    if ($LASTEXITCODE -ne 0) {
        throw "[WIN-SIGN-08] sign_windows.ps1 生成の監査ログ・捕捉アンインストーラ検証が失敗しました: $verify08Output"
    }
    if ($verify08Output -notmatch '検証成功:.*nsu1234\.exe') {
        throw "[WIN-SIGN-08] 捕捉済みアンインストーラ (nsu1234.exe) の検証成功出力が確認できません: $verify08Output"
    }
    $okUninstId = '22222222222222222222222222222222'

    $auditUnsignedCapDir = Join-Path $sandbox 'audit-unsigned-cap'
    New-Item -ItemType Directory -Path $auditUnsignedCapDir -Force | Out-Null
    [System.IO.File]::WriteAllText((Join-Path $auditUnsignedCapDir "start-$okMainId.json"), (@{ id = $okMainId; target = $signToolPath; leaf = 'signtool.exe' } | ConvertTo-Json), $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $auditUnsignedCapDir "ok-$okMainId.json"), (@{ id = $okMainId; target = $signToolPath; leaf = 'signtool.exe'; capturedPath = '' } | ConvertTo-Json), $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $auditUnsignedCapDir "start-$okUninstId.json"), (@{ id = $okUninstId; target = $unsignedPe; leaf = 'unsigned_valid_pe.exe' } | ConvertTo-Json), $utf8NoBom)
    [System.IO.File]::WriteAllText((Join-Path $auditUnsignedCapDir "ok-$okUninstId.json"), (@{ id = $okUninstId; target = $unsignedPe; leaf = 'unsigned_valid_pe.exe'; capturedPath = $unsignedPe } | ConvertTo-Json), $utf8NoBom)
    $res08c = Invoke-PsScript -ScriptPath $verifyScript -ScriptArgs @('-TargetPaths', $signToolPath, '-AuditDir', $auditUnsignedCapDir)
    if ($res08c.ExitCode -eq 0) {
        throw "[WIN-SIGN-08] 未署名のキャプチャ済みアンインストーラで verify_windows_signature.ps1 が成功しました。"
    }
    if ($res08c.Output -notmatch 'No signature found') {
        throw "[WIN-SIGN-08] 未署名キャプチャの拒否理由が想定外です: $($res08c.Output)"
    }
    Write-Host "[WIN-SIGN-08] OK: アンインストーラ欠落拒否・未署名キャプチャ拒否・署名済みキャプチャ通過を確認"

    # WIN-SIGN-09: 複数証明書 PFX が Cert:\CurrentUser\My へインポートされず事前拒否され残存ゼロであること
    $rootCerts = @(Get-ChildItem -Path 'Cert:\LocalMachine\Root' | Select-Object -First 2)
    if ($rootCerts.Count -lt 2) {
        throw "[WIN-SIGN-09] テスト用の公開ルート証明書を 2 件取得できませんでした。"
    }
    $multiCol = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2Collection
    $multiThumbs = @()
    foreach ($rc in $rootCerts) {
        [void]$multiCol.Add($rc)
        $multiThumbs += $rc.Thumbprint.ToUpperInvariant()
    }
    $multiPfxBytes = $multiCol.Export([System.Security.Cryptography.X509Certificates.X509ContentType]::Pkcs12, 'ci-multi-cert-test')
    $multiPfxB64 = [Convert]::ToBase64String($multiPfxBytes)
    $isolatedRunnerTemp = Join-Path $sandbox 'runner-temp'
    New-Item -ItemType Directory -Path $isolatedRunnerTemp -Force | Out-Null

    $res09 = Invoke-PsScript -ScriptPath $buildScript -EnvOverrides @{
        CODE_SIGN_PFX_BASE64      = $multiPfxB64
        CODE_SIGN_PFX_PASSWORD    = 'ci-multi-cert-test'
        TAURI_SIGNING_PRIVATE_KEY = 'dummy-tauri-key-for-pfx-precheck'
        RUNNER_TEMP               = $isolatedRunnerTemp
        GITHUB_ENV                = $null
    }
    if ($res09.ExitCode -eq 0) {
        throw "[WIN-SIGN-09] 複数証明書 PFX で build_and_sign_windows.ps1 が成功しました。"
    }
    if ($res09.Output -notmatch '2') {
        throw "[WIN-SIGN-09] 複数証明書拒否エラーが確認できません: $($res09.Output)"
    }
    foreach ($tp in $multiThumbs) {
        if (Test-Path -LiteralPath "Cert:\CurrentUser\My\$tp") {
            throw "[WIN-SIGN-09] 拒否した複数証明書 PFX の証明書が Cert:\CurrentUser\My に残存しています: $tp"
        }
    }
    $leftoverDirs = @(Get-ChildItem -LiteralPath $isolatedRunnerTemp -Filter 'codesign-*' -Directory -ErrorAction SilentlyContinue)
    if ($leftoverDirs.Count -gt 0) {
        throw "[WIN-SIGN-09] 一時ディレクトリ codesign-* が残存しています。"
    }
    Write-Host "[WIN-SIGN-09] OK: 複数証明書 PFX をインポート前に拒否しストア・一時領域ともに残存ゼロを確認"

    # WIN-SIGN-10: cleanup_windows_signing.ps1 が残存 codesign-* と一時 .pfx を掃討すること
    $cleanupScript = Join-Path $repoRoot 'scripts\cleanup_windows_signing.ps1'
    $leftoverDir = Join-Path $isolatedRunnerTemp 'codesign-leftover-test'
    New-Item -ItemType Directory -Path $leftoverDir -Force | Out-Null
    [System.IO.File]::WriteAllBytes((Join-Path $leftoverDir 'codesign.pfx'), [byte[]]@(1, 2, 3, 4))
    $res10 = Invoke-PsScript -ScriptPath $cleanupScript -EnvOverrides @{
        RUNNER_TEMP                   = $isolatedRunnerTemp
        CODE_SIGN_CLEANUP_THUMBPRINTS = $null
        CODE_SIGN_CLEANUP_THUMBPRINT  = $null
    }
    if ($res10.ExitCode -ne 0) {
        throw "[WIN-SIGN-10] cleanup_windows_signing.ps1 が失敗しました (exit=$($res10.ExitCode)): $($res10.Output)"
    }
    if (Test-Path -LiteralPath $leftoverDir) {
        throw "[WIN-SIGN-10] cleanup_windows_signing.ps1 が codesign-leftover-test を削除できませんでした。"
    }
    Write-Host "[WIN-SIGN-10] OK: cleanup_windows_signing.ps1 の残存一時ディレクトリ掃討を確認"
} finally {
    if (Test-Path -LiteralPath $sandbox) {
        Remove-Item -LiteralPath $sandbox -Recurse -Force -ErrorAction SilentlyContinue
    }
}

# 意図的に失敗させた子プロセスの終了コードが GitHub Actions ステップ結果へ漏れないようリセットする
$global:LASTEXITCODE = 0
exit 0
