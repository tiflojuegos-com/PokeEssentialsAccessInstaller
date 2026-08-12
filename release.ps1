# Builds the launcher and attaches it to the mod's GitHub release.
#
# Refuses to upload when Cargo.toml and the mod's version.json disagree on the launcher version.
# That pair is the whole update mechanism: the launcher compares version.json's "launcher" field
# against its own build version, so a mismatch either hides a real update forever or announces a
# phantom one on every boot. The asset name is fixed because the launcher looks it up by name.
param(
    [Parameter(Mandatory = $true)][string]$Tag,
    [string]$VersionJson = (Join-Path $PSScriptRoot "..\PokeEssentialsAccess\version.json"),
    [string]$Repo = "tiflojuegos-com/PokeEssentialsAccess",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"
$asset = "pokeessentialsaccess-launcher.exe"

function Fail($message) {
    Write-Host "[ERROR] $message" -ForegroundColor Red
    exit 1
}

$cargo = Get-Content (Join-Path $PSScriptRoot "Cargo.toml") -Raw
if ($cargo -notmatch '(?m)^version\s*=\s*"([^"]+)"') { Fail "no encuentro la version en Cargo.toml" }
$built = $Matches[1]

if (-not (Test-Path $VersionJson)) { Fail "no encuentro $VersionJson (pasa -VersionJson)" }
$declared = (Get-Content $VersionJson -Raw | ConvertFrom-Json).launcher
if ([string]::IsNullOrWhiteSpace($declared)) {
    Fail "version.json no declara 'launcher': el instalador nunca ofreceria esta version"
}
if ($declared -ne $built) {
    Fail "descuadre de version: Cargo.toml dice $built y version.json dice $declared"
}

if (-not (gh release view $Tag --repo $Repo 2>$null)) { Fail "la release $Tag no existe en $Repo" }

if (-not $SkipBuild) {
    Write-Host "Compilando $built..."
    cargo build --release
    if ($LASTEXITCODE -ne 0) { Fail "cargo build fallo" }
}

$exe = Join-Path $PSScriptRoot "target\x86_64-pc-windows-msvc\release\$asset"
if (-not (Test-Path $exe)) { $exe = Join-Path $PSScriptRoot "target\release\$asset" }
if (-not (Test-Path $exe)) { Fail "no existe $exe" }

Write-Host "Subiendo $asset ($built) a $Tag..."
gh release upload $Tag $exe --repo $Repo --clobber
if ($LASTEXITCODE -ne 0) { Fail "gh release upload fallo" }

Write-Host "[OK] instalador $built publicado en $Tag" -ForegroundColor Green
