# One-command Windows release build for Orchestrator (M5).
# Runs tests, downloads/verifies CLIProxyAPI sidecar, builds UI + Tauri NSIS/MSI,
# optional smoke launch of the packaged exe.
#
# Usage (from repo root):
#   powershell -ExecutionPolicy Bypass -File scripts\release.ps1
#   powershell -File scripts\release.ps1 -SkipTests
#   powershell -File scripts\release.ps1 -SkipSmoke
param(
  [switch]$SkipTests,
  [switch]$SkipSmoke,
  [switch]$SkipDownload
)

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path $PSScriptRoot -Parent
Set-Location $RepoRoot

function Assert-Command($Name) {
  if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
    throw "Required command not found on PATH: $Name"
  }
}

Write-Host "=== Orchestrator release build ===" -ForegroundColor Cyan
Write-Host "Repo: $RepoRoot"

$vcvars = "${env:ProgramFiles(x86)}\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if (Test-Path $vcvars) {
  cmd /c "`"$vcvars`" && set" | ForEach-Object {
    if ($_ -match '^([^=]+)=(.*)$') {
      [System.Environment]::SetEnvironmentVariable($matches[1], $matches[2], 'Process')
    }
  }
}

Assert-Command cargo
Assert-Command npm
$env:Path = "$env:USERPROFILE\.cargo\bin;$env:Path"

if (-not $SkipTests) {
  Write-Host ""
  Write-Host "=== cargo test (core) ===" -ForegroundColor Cyan
  cargo test
  if ($LASTEXITCODE -ne 0) { throw "cargo test failed" }

  Write-Host ""
  Write-Host "=== cargo test (src-tauri) ===" -ForegroundColor Cyan
  Push-Location src-tauri
  cargo test
  if ($LASTEXITCODE -ne 0) { Pop-Location; throw "src-tauri cargo test failed" }
  Pop-Location
} else {
  Write-Host "Skipping tests (-SkipTests)" -ForegroundColor Yellow
}

if (-not $SkipDownload) {
  Write-Host ""
  Write-Host "=== download + verify CLIProxyAPI sidecar ===" -ForegroundColor Cyan
  & "$PSScriptRoot\download-cliproxy.ps1"
  if ($LASTEXITCODE -ne 0) { throw "download-cliproxy.ps1 failed" }
} else {
  Write-Host "Skipping sidecar download (-SkipDownload)" -ForegroundColor Yellow
}

$triple = Join-Path $RepoRoot "src-tauri\binaries\cli-proxy-api-x86_64-pc-windows-msvc.exe"
if (-not (Test-Path $triple)) {
  throw "Missing sidecar for bundling: $triple - run download-cliproxy.ps1"
}

Write-Host ""
Write-Host "=== npm install + UI build ===" -ForegroundColor Cyan
Push-Location ui
if (Test-Path package-lock.json) {
  npm ci
  if ($LASTEXITCODE -ne 0) { npm install }
} else {
  npm install
}
npm run build
if ($LASTEXITCODE -ne 0) { Pop-Location; throw "UI build failed" }
Pop-Location

Write-Host ""
Write-Host "=== Tauri bundle (NSIS + MSI) ===" -ForegroundColor Cyan
# Prefer loading private key content (createUpdaterArtifacts is picky about PATH alone).
$defaultKey = Join-Path $env:USERPROFILE ".tauri\orchestrator\orchestrator.key"
if (-not $env:TAURI_SIGNING_PRIVATE_KEY) {
  if ($env:TAURI_SIGNING_PRIVATE_KEY_PATH -and (Test-Path $env:TAURI_SIGNING_PRIVATE_KEY_PATH)) {
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $env:TAURI_SIGNING_PRIVATE_KEY_PATH -Raw).Trim()
  } elseif ((Test-Path $defaultKey) -and $env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD) {
    # Only load the key when its password is provided non-interactively;
    # a password-protected key without one makes the CLI prompt and hang
    # unattended builds (CI passes both via secrets).
    $env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content $defaultKey -Raw).Trim()
    Write-Host "Loaded updater private key from $defaultKey"
  } else {
    Write-Host "Note: no updater private key - installer still builds; updater .sig may be skipped." -ForegroundColor Yellow
    Write-Host "See docs/UPDATER.md" -ForegroundColor Yellow
  }
}

# UI is pre-built above; tauri.conf beforeBuildCommand is empty to avoid cwd issues.
# Discover project from repo root (src-tauri/tauri.conf.json).
npx --yes @tauri-apps/cli build --bundles nsis,msi
$buildCode = $LASTEXITCODE
if ($buildCode -ne 0) { throw "tauri build failed with exit $buildCode" }

$bundleDir = Join-Path $RepoRoot "src-tauri\target\release\bundle"
Write-Host ""
Write-Host "=== Bundle outputs ===" -ForegroundColor Cyan
if (Test-Path $bundleDir) {
  Get-ChildItem $bundleDir -Recurse -Include *.exe,*.msi | ForEach-Object {
    $mb = [math]::Round($_.Length / 1MB, 2)
    Write-Host ("  {0}  ({1} MB)" -f $_.FullName, $mb)
  }
} else {
  Write-Host "Bundle dir not found: $bundleDir" -ForegroundColor Red
}

$nsis = Get-ChildItem (Join-Path $bundleDir "nsis") -Filter "*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
$msi = Get-ChildItem (Join-Path $bundleDir "msi") -Filter "*.msi" -ErrorAction SilentlyContinue | Select-Object -First 1
$appExe = Join-Path $RepoRoot "src-tauri\target\release\orchestrator-app.exe"

if (-not $SkipSmoke) {
  Write-Host ""
  Write-Host "=== Smoke: launch release binary, hit /health ===" -ForegroundColor Cyan
  if (-not (Test-Path $appExe)) {
    Write-Host "Release app binary missing at $appExe - smoke skipped" -ForegroundColor Yellow
  } else {
    Get-NetTCPConnection -LocalPort 7420 -ErrorAction SilentlyContinue | ForEach-Object {
      Stop-Process -Id $_.OwningProcess -Force -ErrorAction SilentlyContinue
    }
    $proc = Start-Process -FilePath $appExe -WorkingDirectory $RepoRoot -PassThru -WindowStyle Hidden
    $ok = $false
    for ($i = 0; $i -lt 30; $i++) {
      Start-Sleep -Milliseconds 500
      try {
        $r = Invoke-WebRequest -Uri "http://127.0.0.1:7420/health" -UseBasicParsing -TimeoutSec 2
        if ($r.StatusCode -eq 200) {
          Write-Host "HEALTH OK: $($r.Content)"
          $ok = $true
          break
        }
      } catch { }
      if ($proc.HasExited) {
        Write-Host "Process exited early with code $($proc.ExitCode)" -ForegroundColor Red
        break
      }
    }
    if (-not $proc.HasExited) { Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue }
    if (-not $ok) {
      Write-Host "Smoke health check failed - verify manually if needed" -ForegroundColor Yellow
    }
  }
} else {
  Write-Host "Skipping smoke (-SkipSmoke)" -ForegroundColor Yellow
}

Write-Host ""
Write-Host "=== Done ===" -ForegroundColor Green
if ($nsis) { Write-Host "NSIS installer: $($nsis.FullName)" }
if ($msi) { Write-Host "MSI installer:  $($msi.FullName)" }
Write-Host "App binary:     $appExe"
