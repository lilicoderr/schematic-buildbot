# Downloads the vanilla server jar + official Mojang mappings for a version into a cache OUTSIDE the repo,
# then remaps and decompiles it. Outputs are never committed (see .gitignore / docs/RE/README.md).
param([string]$Version = "1.21.1", [string]$Cache = "$env:LOCALAPPDATA\mc-re-cache")
$ErrorActionPreference = "Stop"
$dir = Join-Path $Cache $Version
New-Item -ItemType Directory -Force $dir | Out-Null
$manifest = Invoke-RestMethod "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
if ($Version -eq "latest") { $Version = $manifest.latest.release; $dir = Join-Path $Cache $Version; New-Item -ItemType Directory -Force $dir | Out-Null }
$entry = $manifest.versions | Where-Object id -eq $Version
$vj = Invoke-RestMethod $entry.url
Invoke-WebRequest $vj.downloads.server.url -OutFile "$dir\server.jar"
Invoke-WebRequest $vj.downloads.server_mappings.url -OutFile "$dir\server_mappings.txt"
$vj | ConvertTo-Json -Depth 20 | Set-Content "$dir\version.json"
Write-Host "downloaded $Version to $dir"
