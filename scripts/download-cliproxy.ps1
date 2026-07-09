# Download and extract the pinned CLIProxyAPI Windows x64 sidecar.
# Pin is recorded in src-tauri/binaries/VERSION.txt
$ErrorActionPreference = "Stop"
$root = Split-Path (Split-Path $PSScriptRoot -Parent) -ErrorAction SilentlyContinue
if (-not $root) { $root = "C:\Users\Ben\Desktop\Orchestrator" }
# script is scripts/ under repo
$repo = Split-Path $PSScriptRoot -Parent
$binDir = Join-Path $repo "src-tauri\binaries"
$versionFile = Join-Path $binDir "VERSION.txt"
New-Item -ItemType Directory -Force -Path $binDir | Out-Null

$url = "https://github.com/router-for-me/CLIProxyAPI/releases/download/v7.2.58/CLIProxyAPI_7.2.58_windows_amd64.zip"
$zip = Join-Path $binDir "CLIProxyAPI_7.2.58_windows_amd64.zip"
$expected = "e45be3e1743a530d01c1b9bf97e282e00bf23c7d4ba734cd9ef1b04e66936ff1"

Write-Host "Downloading $url"
Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
$hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
if ($hash -ne $expected) {
  throw "SHA256 mismatch: got $hash expected $expected"
}
$extract = Join-Path $binDir "extract"
if (Test-Path $extract) { Remove-Item $extract -Recurse -Force }
Expand-Archive -Path $zip -DestinationPath $extract -Force
Copy-Item (Join-Path $extract "cli-proxy-api.exe") (Join-Path $binDir "cli-proxy-api.exe") -Force
Write-Host "Installed: $(Join-Path $binDir 'cli-proxy-api.exe')"
Write-Host "Pin: see $versionFile"
