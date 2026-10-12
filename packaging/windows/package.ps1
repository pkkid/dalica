<#
.SYNOPSIS
  Build and package Dalica for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    dalica-<version>-windows-<arch>-portable.zip   dalica.exe + dalica-cli.exe + the portable
                                                   marker + the licence files

  A stub (M6.13): it packs the release binaries into the portable zip so the release workflow
  runs whole; M6.22 adds the MSI, the icon and VERSIONINFO and the GUI subsystem, and M15 signs
  (docs/releasing.md -> Windows). The binaries link the C runtime statically (+crt-static), so
  nothing needs the Visual C++ redistributable.

  Adapted from PhotoCraft packaging/windows/package.ps1 @ 2db6b7d8 (MIT OR Apache-2.0): the
  version and provenance lookup, Invoke-Native, the target-scoped static CRT flag, cut down to the
  portable zip.

  Environment (all optional): DALICA_VERSION (default: [workspace.package] version of
  Cargo.toml), DALICA_BUILD_SHA (default: git HEAD), DALICA_BUILD_DATE (default: today, UTC),
  DIST, CARGO_TARGET_DIR, CRAFT_FONTS_DIR (its fonts' OFL.txt files ship as OFL-<family>.txt).

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch x64 -SkipBuild
#>
param(
  [ValidateSet('x64')] [string] $Arch = 'x64',
  [switch] $SkipBuild
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

# The version lives in one place: [workspace.package] version in the root Cargo.toml.
$Version = $env:DALICA_VERSION
if (-not $Version) {
  $inPkg = $false
  foreach ($line in Get-Content (Join-Path $Root 'Cargo.toml')) {
    if ($line -match '^\s*\[') { $inPkg = ($line.Trim() -eq '[workspace.package]'); continue }
    if ($inPkg -and $line -match '^\s*version\s*=\s*"([^"]+)"') { $Version = $Matches[1]; break }
  }
}
if (-not $Version) { throw 'could not read [workspace.package] version from Cargo.toml' }

# Provenance baked into the binaries (crates/core/src/build.rs).
if (-not $env:DALICA_BUILD_SHA) { $env:DALICA_BUILD_SHA = (git -C $Root rev-parse HEAD 2>$null) }
if (-not $env:DALICA_BUILD_DATE) { $env:DALICA_BUILD_DATE = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd') }
$ShortSha = if ($env:DALICA_BUILD_SHA) { $env:DALICA_BUILD_SHA.Substring(0, [Math]::Min(9, $env:DALICA_BUILD_SHA.Length)) } else { '' }

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
Write-Output "Dalica $Version for Windows $Arch ($Target, $ShortSha, $env:DALICA_BUILD_DATE)"

if (-not $SkipBuild) {
  # Static CRT: no VC++ redistributable needed. Scoped to the target so host build scripts and
  # proc-macros are unaffected.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p dalica -p dalica-cli --target $Target }
}
$Bin = Join-Path $TargetDir "$Target\release"

# Both binaries must report this version and the commit, not a dev build.
foreach ($exe in @('dalica.exe', 'dalica-cli.exe')) {
  $out = (& (Join-Path $Bin $exe) --version | Out-String).Trim()
  if ($LASTEXITCODE -ne 0) { throw "$exe --version failed with exit code $LASTEXITCODE" }
  Write-Output "$exe --version: $out"
  if (-not $out.Contains("$Version (")) { throw "$exe reports '$out', not $Version" }
  if ($ShortSha -and -not $out.Contains($ShortSha)) { throw "$exe does not report commit $ShortSha" }
}

# ---- portable zip ------------------------------------------------------------------------------
$Name = "dalica-$Version-windows-$Arch-portable"
$Stage = Join-Path $TargetDir "windows-package\$Name"
Remove-Item -Recurse -Force (Split-Path $Stage) -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $Stage | Out-Null
Copy-Item (Join-Path $Bin 'dalica.exe'), (Join-Path $Bin 'dalica-cli.exe') $Stage
# The marker beside the exe keeps settings, the index and caches next to it (portable mode,
# apps/dalica/src/paths.rs). It is an empty file named `portable`.
New-Item -ItemType File -Force -Path (Join-Path $Stage 'portable') | Out-Null
foreach ($f in @('LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md', 'README.md')) {
  Copy-Item (Join-Path $Root $f) $Stage
}
Copy-Item (Join-Path $Root 'assets\fonts\OFL-Inter.txt') $Stage
if ($env:CRAFT_FONTS_DIR) {
  foreach ($lic in Get-ChildItem -Path (Join-Path $env:CRAFT_FONTS_DIR 'fonts\*\OFL.txt') -ErrorAction SilentlyContinue) {
    Copy-Item $lic.FullName (Join-Path $Stage "OFL-$($lic.Directory.Name).txt")
  }
}

$Zip = Join-Path $Dist "$Name.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Stage -DestinationPath $Zip
Write-Output "wrote $Zip"
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($Zip)
try { $archive.Entries | ForEach-Object { Write-Output "  $($_.FullName) ($($_.Length) bytes)" } } finally { $archive.Dispose() }
