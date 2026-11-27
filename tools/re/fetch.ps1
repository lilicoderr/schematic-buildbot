# Downloads the vanilla server jar + official Mojang mappings for a version into a cache OUTSIDE the repo,
# then remaps and decompiles it. Outputs are never committed (see .gitignore / docs/RE/README.md).
param([string]$Version = "1.21.1", [string]$Cache = "$env:LOCALAPPDATA\mc-re-cache")
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"
$dir = Join-Path $Cache $Version
New-Item -ItemType Directory -Force $dir | Out-Null
$manifest = Invoke-RestMethod "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json"
if ($Version -eq "latest") { $Version = $manifest.latest.release; $dir = Join-Path $Cache $Version; New-Item -ItemType Directory -Force $dir | Out-Null }
$entry = $manifest.versions | Where-Object id -eq $Version
if (-not $entry) { throw "unknown version $Version" }
$vj = Invoke-RestMethod $entry.url
function Get-Verified($dl, $out) {
    Invoke-WebRequest $dl.url -OutFile $out -UseBasicParsing
    $hash = (Get-FileHash $out -Algorithm SHA1).Hash
    if ($hash -ne $dl.sha1) { Remove-Item $out; throw "sha1 mismatch for $out" }
}
Get-Verified $vj.downloads.server "$dir\server.jar"
# 26.x releases ship unobfuscated jars: there is no server_mappings download (and nothing to remap).
if ($vj.downloads.server_mappings) { Get-Verified $vj.downloads.server_mappings "$dir\server_mappings.txt" }
$vj | ConvertTo-Json -Depth 20 | Set-Content "$dir\version.json"
Write-Host "downloaded $Version to $dir"
