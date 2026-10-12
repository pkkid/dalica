<#
.SYNOPSIS
  Check apps/dalica/build.rs on Windows: without a resource compiler the build fails with the tool
  named (and no panic); with one, the exe carries the icon and VERSIONINFO, and only a release build
  is a GUI program.

.DESCRIPTION
  Builds a throwaway crate named `dalica` whose build script is a copy of apps/dalica/build.rs, next
  to a copy of the app icon at the same relative path, and an empty `fn main`. That takes seconds
  (only winresource is compiled), unlike a build of the app. Three cases, each in its own target
  folder under a temporary directory:
    1. RC_PATH names an rc.exe that does not exist: cargo fails, the output names that rc.exe and
       says "resource compiler", and nothing panicked;
    2. the Windows SDK's rc.exe, release: the exe's version resource says Dalica and the version,
       its numeric file version is X.Y.Z.0, it has an icon, and its PE subsystem is 2 (GUI);
    3. the same, debug: PE subsystem 3 (console).
  Needs Windows, Rust with the MSVC target, and the Windows SDK. package.ps1 runs it in CI.

.EXAMPLE
  pwsh packaging/windows/test-build-rs.ps1 -Version 0.1.0
#>
param(
  [Parameter(Mandatory = $true)] [string] $Version,
  [string] $Target = 'x86_64-pc-windows-msvc'
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
. (Join-Path $PSScriptRoot 'pe.ps1')

# The winresource version the app builds with, from the lockfile, so the probe compiles the same code.
$lock = Get-Content (Join-Path $Root 'Cargo.lock') -Raw
if ($lock -notmatch '(?m)^name = "winresource"\r?\nversion = "([^"]+)"') { throw 'winresource is not in Cargo.lock' }
$Winres = $Matches[1]
$WantVersion = ($Version -split '-')[0]

$tmp = Join-Path ([IO.Path]::GetTempPath()) "dalica-build-rs-$([Guid]::NewGuid().ToString('N'))"
$crate = Join-Path $tmp 'apps\dalica'
New-Item -ItemType Directory -Force -Path (Join-Path $crate 'src'), (Join-Path $tmp 'assets\app-icon') | Out-Null
Copy-Item (Join-Path $Root 'apps\dalica\build.rs') $crate
Copy-Item (Join-Path $Root 'assets\app-icon\dalica.ico') (Join-Path $tmp 'assets\app-icon')
Set-Content -Path (Join-Path $crate 'src\main.rs') -Value 'fn main() {}' -Encoding utf8
Set-Content -Path (Join-Path $crate 'Cargo.toml') -Encoding utf8 -Value @"
[package]
name = "dalica"
version = "$Version"
edition = "2024"
publish = false

[build-dependencies]
winresource = { version = "=$Winres", default-features = false }

[workspace]
"@

# Runs cargo in the probe crate; returns its exit code and everything it printed.
function Invoke-Probe([string] $Name, [string[]] $CargoArgs, [hashtable] $Vars = @{}) {
  $saved = @{}
  foreach ($k in @('RC_PATH', 'CARGO_TARGET_DIR') + @($Vars.Keys)) { $saved[$k] = [Environment]::GetEnvironmentVariable($k) }
  try {
    [Environment]::SetEnvironmentVariable('RC_PATH', $null)
    [Environment]::SetEnvironmentVariable('CARGO_TARGET_DIR', (Join-Path $tmp "target-$Name"))
    foreach ($k in $Vars.Keys) { [Environment]::SetEnvironmentVariable($k, $Vars[$k]) }
    Push-Location $crate
    # cargo reports progress on standard error; an expected failure must not stop this script.
    $eap = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
      $out = (& cargo build --target $Target @CargoArgs 2>&1 | Out-String)
      $code = $LASTEXITCODE
    } finally { $ErrorActionPreference = $eap; Pop-Location }
  } finally {
    foreach ($k in $saved.Keys) { [Environment]::SetEnvironmentVariable($k, $saved[$k]) }
  }
  return @{ Code = $code; Out = $out }
}

$passed = $false
try {
  # 1. No resource compiler: an error that names it, never a panic.
  $missing = Join-Path $tmp 'no-sdk\rc.exe'
  $r = Invoke-Probe 'missing' @('--release') @{ RC_PATH = $missing }
  if ($r.Code -eq 0) { Write-Output $r.Out; throw 'build.rs: the build succeeded without a resource compiler' }
  if (-not $r.Out.Contains($missing)) { Write-Output $r.Out; throw "build.rs: the error does not name the missing tool $missing" }
  if (-not $r.Out.Contains('resource compiler')) { Write-Output $r.Out; throw "build.rs: the error does not say 'resource compiler'" }
  if ($r.Out -match 'panicked') { Write-Output $r.Out; throw 'build.rs panicked instead of returning an error' }
  Write-Output "ok build.rs without a resource compiler: fails naming $missing"

  # 2. Release with the SDK's rc.exe: the resource is there, and the program is a GUI one.
  $r = Invoke-Probe 'release' @('--release')
  if ($r.Code -ne 0) { Write-Output $r.Out; throw "build.rs: the release build failed (exit $($r.Code))" }
  $exe = Join-Path $tmp "target-release\$Target\release\dalica.exe"
  $info = (Get-Item $exe).VersionInfo
  if ($info.ProductName -ne 'Dalica') { throw "build.rs: ProductName is '$($info.ProductName)', not Dalica" }
  if ($info.FileDescription -ne 'Dalica') { throw "build.rs: FileDescription is '$($info.FileDescription)', not Dalica" }
  if ($info.ProductVersion -ne $Version) { throw "build.rs: ProductVersion is '$($info.ProductVersion)', not $Version" }
  $fileVersion = "$($info.FileMajorPart).$($info.FileMinorPart).$($info.FileBuildPart)"
  if ($fileVersion -ne $WantVersion -or $info.FilePrivatePart -ne 0) { throw "build.rs: the numeric file version is $fileVersion.$($info.FilePrivatePart), not $WantVersion.0" }
  if (-not (Test-EmbeddedIcon $exe (Join-Path $Root 'assets\app-icon\dalica.ico'))) { throw 'build.rs: the exe does not carry the app icon' }
  $h = Get-PeHeader $exe
  if ($h.Subsystem -ne 2) { throw "build.rs: a release build has PE subsystem $($h.Subsystem), expected 2 (GUI)" }
  Write-Output "ok build.rs release: Dalica $($info.ProductVersion) ($fileVersion.0), icon, GUI subsystem"

  # 3. Debug: still a console program, so tests and `cargo run` keep their output.
  $r = Invoke-Probe 'debug' @()
  if ($r.Code -ne 0) { Write-Output $r.Out; throw "build.rs: the debug build failed (exit $($r.Code))" }
  $h = Get-PeHeader (Join-Path $tmp "target-debug\$Target\debug\dalica.exe")
  if ($h.Subsystem -ne 3) { throw "build.rs: a debug build has PE subsystem $($h.Subsystem), expected 3 (console)" }
  Write-Output 'ok build.rs debug: console subsystem'
  $passed = $true
} finally {
  if ($passed) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue } else { Write-Output "test files kept in $tmp" }
}
