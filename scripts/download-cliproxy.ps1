# Download and extract the pinned CLIProxyAPI Windows x64 sidecar.
# Pin + checksum: src-tauri/binaries/VERSION.txt
# Also stages the Tauri externalBin name: cli-proxy-api-x86_64-pc-windows-msvc.exe
$ErrorActionPreference = "Stop"
$repo = Split-Path $PSScriptRoot -Parent
$binDir = Join-Path $repo "src-tauri\binaries"
$versionFile = Join-Path $binDir "VERSION.txt"
New-Item -ItemType Directory -Force -Path $binDir | Out-Null

if (-not (Test-Path $versionFile)) {
  throw "Missing VERSION.txt at $versionFile"
}

$versionLines = Get-Content $versionFile
$url = ($versionLines | Where-Object { $_ -match '^windows_amd64_url=(.+)$' } | ForEach-Object { $Matches[1] } | Select-Object -First 1)
$expected = ($versionLines | Where-Object { $_ -match '^windows_amd64_sha256=(.+)$' } | ForEach-Object { $Matches[1] } | Select-Object -First 1)
if (-not $url -or -not $expected) {
  throw "VERSION.txt missing windows_amd64_url or windows_amd64_sha256"
}
$expected = $expected.ToLower()

$zipName = Split-Path $url -Leaf
$zip = Join-Path $binDir $zipName
$destExe = Join-Path $binDir "cli-proxy-api.exe"
$destTriple = Join-Path $binDir "cli-proxy-api-x86_64-pc-windows-msvc.exe"

# Reuse existing verified zip if present
if (Test-Path $zip) {
  $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
  if ($hash -ne $expected) {
    Write-Host "Existing zip hash mismatch; re-downloading..."
    Remove-Item $zip -Force
  }
}

if (-not (Test-Path $zip)) {
  Write-Host "Downloading $url"
  Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
}

$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
if ($hash -ne $expected) {
  throw "SHA256 mismatch: got $hash expected $expected"
}
Write-Host "Checksum OK: $hash"

$extract = Join-Path $binDir "extract"
if (Test-Path $extract) { Remove-Item $extract -Recurse -Force }
Expand-Archive -Path $zip -DestinationPath $extract -Force
$srcExe = Join-Path $extract "cli-proxy-api.exe"
if (-not (Test-Path $srcExe)) {
  throw "cli-proxy-api.exe not found inside zip"
}
Copy-Item $srcExe $destExe -Force
Copy-Item $srcExe $destTriple -Force
Write-Host "Installed: $destExe"
Write-Host "Staged for Tauri externalBin: $destTriple"
Write-Host "Pin: see $versionFile"
