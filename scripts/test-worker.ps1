# Prompt whatever model is currently in the Orchestrator "worker" slot.
# Usage:
#   powershell -File scripts\test-worker.ps1
#   powershell -File scripts\test-worker.ps1 "Write a haiku about routers"
#   powershell -File scripts\test-worker.ps1 -Prompt "hi" -ApiKey "proxy-..."

param(
    [Parameter(Position = 0)]
    [string]$Prompt = "Reply with exactly: worker-ok",

    [string]$SlotsPath = "$env:APPDATA\Orchestrator\slots.json",
    [string]$CliproxySettings = "$env:APPDATA\Orchestrator\cliproxy\settings.json",
    [string]$ApiKey = ""
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $SlotsPath)) {
    throw "slots.json not found: $SlotsPath (is Orchestrator installed/running?)"
}

$slots = Get-Content $SlotsPath -Raw | ConvertFrom-Json
$worker = $slots.slots.worker
if (-not $worker) {
    throw "No 'worker' slot in $SlotsPath"
}

$baseUrl = $worker.base_url.TrimEnd("/")
$model = $worker.model
$authRef = $worker.auth_ref

if (-not $ApiKey) {
    if ($authRef -eq "cliproxy_proxy_key" -and (Test-Path $CliproxySettings)) {
        $ApiKey = (Get-Content $CliproxySettings -Raw | ConvertFrom-Json).proxy_api_key
    }
    elseif ($env:ORCHESTRATOR_WORKER_API_KEY) {
        $ApiKey = $env:ORCHESTRATOR_WORKER_API_KEY
    }
}

if (-not $ApiKey) {
    throw "No API key. Pass -ApiKey, set ORCHESTRATOR_WORKER_API_KEY, or use cliproxy_proxy_key."
}

$url = "$baseUrl/chat/completions"
$body = @{
    model = $model
    stream = $false
    messages = @(
        @{ role = "user"; content = $Prompt }
    )
} | ConvertTo-Json -Depth 5

Write-Host "Worker model : $model"
Write-Host "Endpoint     : $url"
Write-Host "Prompt       : $Prompt"
Write-Host "---"

$headers = @{
    Authorization = "Bearer $ApiKey"
    "Content-Type" = "application/json"
}

$resp = Invoke-RestMethod -Uri $url -Method Post -Headers $headers -Body $body -TimeoutSec 120
$text = $resp.choices[0].message.content
Write-Host $text
if ($resp.usage) {
    Write-Host "---"
    Write-Host ("usage: prompt={0} completion={1} total={2}" -f `
        $resp.usage.prompt_tokens, $resp.usage.completion_tokens, $resp.usage.total_tokens)
}
