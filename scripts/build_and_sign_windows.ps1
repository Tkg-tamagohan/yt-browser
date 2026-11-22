#Requires -Version 5.1
<#
.SYNOPSIS
release.yml の Windows ビルド・Authenticode 署名・署名検証を実行する。

.DESCRIPTION
環境変数 CODE_SIGN_PFX_BASE64 / CODE_SIGN_PFX_PASSWORD / TAURI_SIGNING_PRIVATE_KEY を受け取り、
PFX の証明書数と秘密鍵有無をメモリ上（EphemeralKeySet）で事前検証してから Cert:\CurrentUser\My へ
非エクスポートでインポートする。ビルド開始前に PFX ファイルと Secrets 環境変数を破棄し、
一時 --config JSON 経由で scripts/sign_windows.ps1 を注入して NSIS ビルドを実行する。
ビルド後は本体・キャプチャ済みアンインストーラ・セットアップのすべてを verify_windows_signature.ps1 で検証し、
finally で証明書ストアと一時領域を確実に掃討する。
#>
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$utf8NoBom = [System.Text.UTF8Encoding]::new($false)
$workDir = ''
$pfxPath = ''
$importedThumbprints = @()

try {
    if ([string]::IsNullOrWhiteSpace($env:CODE_SIGN_PFX_BASE64) -or [string]::IsNullOrWhiteSpace($env:CODE_SIGN_PFX_PASSWORD)) {
        throw "CODE_SIGN_PFX_BASE64 または CODE_SIGN_PFX_PASSWORD が未設定です。"
    }
    if ([string]::IsNullOrWhiteSpace($env:TAURI_SIGNING_PRIVATE_KEY)) {
        throw "TAURI_SIGNING_PRIVATE_KEY が未設定です。"
    }

    $tempRoot = if (-not [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
        $env:RUNNER_TEMP
    } else {
        [System.IO.Path]::GetTempPath()
    }
    $workDir = Join-Path $tempRoot ("codesign-" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $workDir -Force | Out-Null

    $auditDir = Join-Path $workDir 'sign-audit'
    $captureDir = Join-Path $workDir 'sign-capture'
    New-Item -ItemType Directory -Path $auditDir -Force | Out-Null
    New-Item -ItemType Directory -Path $captureDir -Force | Out-Null

    $pfxPath = Join-Path $workDir 'codesign.pfx'
    $cerPath = Join-Path $workDir 'codesign.cer'
    $signConfigPath = Join-Path $workDir 'tauri-sign.json'

    $pfxBytes = [Convert]::FromBase64String($env:CODE_SIGN_PFX_BASE64.Trim())
    try {
        # ストアへインポートする前に、メモリ上（EphemeralKeySet）で証明書数と秘密鍵を検証する
        $inspectCollection = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2Collection
        $ephemeralFlags = [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet
        try {
            $inspectCollection.Import($pfxBytes, $env:CODE_SIGN_PFX_PASSWORD, $ephemeralFlags)
            if ($inspectCollection.Count -ne 1) {
                throw "PFX 内の証明書数が想定外です ($($inspectCollection.Count) 件)。"
            }
            if (-not $inspectCollection[0].HasPrivateKey) {
                throw "PFX 内の証明書に秘密鍵が含まれていません (HasPrivateKey=False)。"
            }
        } finally {
            foreach ($certItem in $inspectCollection) {
                $certItem.Dispose()
            }
        }

        [System.IO.File]::WriteAllBytes($pfxPath, $pfxBytes)
    } finally {
        [Array]::Clear($pfxBytes, 0, $pfxBytes.Length)
    }

    $securePass = ConvertTo-SecureString -String $env:CODE_SIGN_PFX_PASSWORD -AsPlainText -Force
    $importedCerts = @(Import-PfxCertificate -FilePath $pfxPath -CertStoreLocation 'Cert:\CurrentUser\My' -Password $securePass)

    # 件数検証より先に全サムプリントを記録し、例外時でも finally と保険クリーンアップで全件削除する
    $importedThumbprints = @(
        $importedCerts |
            Where-Object { $null -ne $_ -and -not [string]::IsNullOrWhiteSpace($_.Thumbprint) } |
            ForEach-Object { $_.Thumbprint.ToUpperInvariant() } |
            Select-Object -Unique
    )
    if ($importedThumbprints.Count -gt 0 -and -not [string]::IsNullOrWhiteSpace($env:GITHUB_ENV)) {
        $envLines = "CODE_SIGN_CLEANUP_THUMBPRINTS=$($importedThumbprints -join ',')`n"
        if ($importedThumbprints.Count -eq 1) {
            $envLines += "CODE_SIGN_CLEANUP_THUMBPRINT=$($importedThumbprints[0])`n"
        }
        [System.IO.File]::AppendAllText($env:GITHUB_ENV, $envLines, $utf8NoBom)
    }

    if ($importedCerts.Count -ne 1) {
        throw "PFX 内の証明書数が想定外です ($($importedCerts.Count) 件)。"
    }
    $importedCert = $importedCerts[0]
    $thumbprint = $importedCert.Thumbprint.ToUpperInvariant()

    Export-Certificate -Cert $importedCert -FilePath $cerPath -Force | Out-Null

    # ビルド前に PFX ファイルと Secrets 環境変数を破棄し、子プロセスへ渡さない
    $pfxLen = (Get-Item -LiteralPath $pfxPath).Length
    [System.IO.File]::WriteAllBytes($pfxPath, (New-Object byte[] $pfxLen))
    Remove-Item -LiteralPath $pfxPath -Force
    $securePass = $null
    Remove-Item Env:CODE_SIGN_PFX_BASE64 -ErrorAction SilentlyContinue
    Remove-Item Env:CODE_SIGN_PFX_PASSWORD -ErrorAction SilentlyContinue

    $repoRoot = (Get-Location).ProviderPath
    $signScript = (Join-Path $repoRoot 'scripts\sign_windows.ps1').Replace('\', '/')
    $signCfg = @{
        bundle = @{
            windows = @{
                signCommand = @{
                    cmd  = 'powershell.exe'
                    args = @(
                        '-NoProfile',
                        '-NonInteractive',
                        '-ExecutionPolicy',
                        'Bypass',
                        '-File',
                        $signScript,
                        '%1'
                    )
                }
            }
        }
    }
    $signJson = $signCfg | ConvertTo-Json -Depth 10
    [System.IO.File]::WriteAllText($signConfigPath, $signJson, $utf8NoBom)

    $env:CODE_SIGN_CERT_THUMBPRINT = $thumbprint
    $env:CODE_SIGN_AUDIT_DIR = $auditDir
    $env:CODE_SIGN_CAPTURE_DIR = $captureDir

    Write-Host "Tauri Windows ビルドと Authenticode 署名を開始します (Thumbprint=$thumbprint)"
    # Windows PowerShell では pnpm.ps1 が実行ポリシー制約を受け得るため pnpm.cmd を明示する
    & pnpm.cmd tauri build --ci --bundles nsis --config $signConfigPath -- --locked
    if ($LASTEXITCODE -ne 0) {
        throw "pnpm.cmd tauri build が終了コード $LASTEXITCODE で失敗しました。"
    }

    $appExe = Join-Path $repoRoot 'src-tauri\target\release\yt-browser.exe'
    if (-not (Test-Path -LiteralPath $appExe -PathType Leaf)) {
        throw "ビルド済みバイナリが見つかりません: $appExe"
    }
    $nsisDir = Join-Path $repoRoot 'src-tauri\target\release\bundle\nsis'
    $nsisExes = @(Get-ChildItem -Path $nsisDir -Filter '*-setup.exe' -File -ErrorAction SilentlyContinue)
    if ($nsisExes.Count -eq 0) {
        throw "NSIS インストーラが見つかりません: $nsisDir"
    }
    $nsisSigs = @(Get-ChildItem -Path $nsisDir -Filter '*.exe.sig' -File -ErrorAction SilentlyContinue)
    if ($nsisSigs.Count -eq 0) {
        throw "Tauri updater 用署名 (*.exe.sig) が見つかりません: $nsisDir"
    }

    # Tauri v2 の NSIS バンドラは梱包用に yt-browser.exe へ署名したあとビルド終了時に
    # 未パッチ・未署名の原本へ戻すため、アプリ本体とアンインストーラの署名検証は
    # -AuditDir 経由のキャプチャ済み署名バイナリ（各 1 件以上必須）で行う。
    $verifyTargets = @($nsisExes | ForEach-Object { $_.FullName })
    $verifyScript = Join-Path $repoRoot 'scripts\verify_windows_signature.ps1'
    & $verifyScript -TargetPaths $verifyTargets -AuditDir $auditDir -ExpectedThumbprint $thumbprint -TrustCertPath $cerPath
    if ($LASTEXITCODE -ne 0) {
        throw "verify_windows_signature.ps1 が終了コード $LASTEXITCODE で失敗しました。"
    }
} finally {
    Remove-Item Env:CODE_SIGN_PFX_BASE64 -ErrorAction SilentlyContinue
    Remove-Item Env:CODE_SIGN_PFX_PASSWORD -ErrorAction SilentlyContinue
    Remove-Item Env:CODE_SIGN_CERT_THUMBPRINT -ErrorAction SilentlyContinue
    Remove-Item Env:CODE_SIGN_AUDIT_DIR -ErrorAction SilentlyContinue
    Remove-Item Env:CODE_SIGN_CAPTURE_DIR -ErrorAction SilentlyContinue

    if (-not [string]::IsNullOrEmpty($pfxPath) -and (Test-Path -LiteralPath $pfxPath -PathType Leaf)) {
        try {
            $len = (Get-Item -LiteralPath $pfxPath).Length
            [System.IO.File]::WriteAllBytes($pfxPath, (New-Object byte[] $len))
            Remove-Item -LiteralPath $pfxPath -Force
        } catch {
            Write-Warning "一時 PFX の削除に失敗しました: $($_.Exception.Message)"
        }
    }

    if (-not [string]::IsNullOrEmpty($workDir) -and (Test-Path -LiteralPath $workDir)) {
        try {
            Remove-Item -LiteralPath $workDir -Recurse -Force
        } catch {
            Write-Warning "一時作業ディレクトリの削除に失敗しました: $($_.Exception.Message)"
        }
    }

    foreach ($tp in $importedThumbprints) {
        if ([string]::IsNullOrWhiteSpace($tp)) {
            continue
        }
        try {
            $myPath = "Cert:\CurrentUser\My\$tp"
            if (Test-Path -LiteralPath $myPath) {
                Remove-Item -LiteralPath $myPath -Force
            }
            if (Test-Path -LiteralPath $myPath) {
                & certutil.exe -user -delstore My $tp | Out-Null
            }
        } catch {
            Write-Warning "CurrentUser\My の一時証明書削除に失敗しました ($tp): $($_.Exception.Message)"
        }
    }
}

$global:LASTEXITCODE = 0
