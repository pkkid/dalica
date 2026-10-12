<#
.SYNOPSIS
  Build, sign and package Dalica for Windows.

.DESCRIPTION
  Produces, in $env:DIST (default: dist/release):
    dalica-<version>-windows-<arch>.msi            per-machine installer (WiX v5, dalica.wxs)
    dalica-<version>-windows-<arch>-portable.zip   dalica.exe + dalica-cli.exe + the `portable`
                                                   marker + the licence files

  The binaries link the C runtime statically (+crt-static), so neither package needs the Visual
  C++ redistributable. apps/dalica/build.rs embeds the icon and VERSIONINFO into dalica.exe and
  makes the release build a GUI program; this script checks both, and that dalica-cli.exe stays a
  console program. sign.ps1 signs both programs and the MSI when signing secrets are set and warns
  otherwise (docs/releasing.md -> Windows).

  Tests (in CI, or with -Test; they install into Program Files): test-build-rs.ps1 (build.rs with
  and without a resource compiler), test-portable.ps1 (the zip keeps its data beside the exe) and
  test-msi.ps1 (silent install, version resource, same-version upgrade, clean uninstall).

  Tools: Rust with the MSVC target and the Windows SDK (rc.exe, signtool.exe), and WiX v5 with its
  UI and Util extensions. In CI (GITHUB_ACTIONS) or with -InstallTools the script installs WiX
  itself (`dotnet tool install --global wix`); elsewhere a missing WiX skips the MSI and says so.

  Adapted from PhotoCraft packaging/windows/package.ps1 (revisions 2db6b7d8 and 0d6e658e; MIT OR
  Apache-2.0): the version and provenance lookup, Invoke-Native, the target-scoped static CRT
  flag, the PE header checks, the installer art and the WiX build. Changed for Dalica: the
  --version and version-resource checks, the licence files in both packages, WiX installed by
  the script, the tests, and a job summary.

  Environment (all optional): DALICA_VERSION (default: [workspace.package] version of
  Cargo.toml), DALICA_BUILD_SHA (default: git HEAD), DALICA_BUILD_DATE (default: today, UTC),
  DIST, CARGO_TARGET_DIR, CRAFT_FONTS_DIR (its fonts' OFL.txt files ship as OFL-<family>.txt),
  and the signing secrets sign.ps1 reads.

.EXAMPLE
  pwsh packaging/windows/package.ps1 -Arch x64
  pwsh packaging/windows/package.ps1 -Arch x64 -SkipBuild -Test
#>
param(
  [ValidateSet('x64')] [string] $Arch = 'x64',
  [switch] $SkipBuild,
  # Run the install, portable and build.rs tests (default: in CI only).
  [switch] $Test,
  # Never run them, even in CI.
  [switch] $NoTest,
  # Install WiX when it is missing (default: in CI only).
  [switch] $InstallTools
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$InCi = $env:GITHUB_ACTIONS -eq 'true'
$RunTests = -not $NoTest -and ($Test -or $InCi)
. (Join-Path $PSScriptRoot 'pe.ps1')

# The WiX release the MSI is built with (major version 5: dalica.wxs uses its schema and `Files`).
$WixVersion = '5.0.2'

function Invoke-Native([string] $What, [scriptblock] $Block) {
  Write-Output "==> $What"
  & $Block
  if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE" }
}

function Add-Summary([string] $Line) {
  if ($env:GITHUB_STEP_SUMMARY) { Add-Content -Path $env:GITHUB_STEP_SUMMARY -Value $Line -Encoding utf8 }
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
# The MSI ProductVersion is numeric X.Y.Z (a pre-release tag is dropped there), with Windows
# Installer's limits: major and minor up to 255, build up to 65535.
if ($Version -notmatch '^(\d{1,9})\.(\d{1,9})\.(\d{1,9})(-[0-9A-Za-z.-]+)?$') { throw "'$Version' is not a version like 1.2.3 or 1.2.3-rc.1" }
if ([int]$Matches[1] -gt 255 -or [int]$Matches[2] -gt 255 -or [int]$Matches[3] -gt 65535) {
  throw "$Version does not fit an MSI version (major and minor up to 255, patch up to 65535)"
}
$MsiVersion = "$([int]$Matches[1]).$([int]$Matches[2]).$([int]$Matches[3])"

# Provenance baked into the binaries (crates/core/src/build.rs).
if (-not $env:DALICA_BUILD_SHA) { $env:DALICA_BUILD_SHA = (git -C $Root rev-parse HEAD 2>$null) }
if (-not $env:DALICA_BUILD_DATE) { $env:DALICA_BUILD_DATE = (Get-Date).ToUniversalTime().ToString('yyyy-MM-dd') }
$ShortSha = if ($env:DALICA_BUILD_SHA) { $env:DALICA_BUILD_SHA.Substring(0, [Math]::Min(9, $env:DALICA_BUILD_SHA.Length)) } else { '' }

$Target = switch ($Arch) { 'x64' { 'x86_64-pc-windows-msvc' } }
$Machine = switch ($Arch) { 'x64' { 0x8664 } }
$Dist = if ($env:DIST) { $env:DIST } else { Join-Path $Root 'dist\release' }
$TargetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $Root 'target' }
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
Write-Output "Dalica $Version for Windows $Arch ($Target, $ShortSha, $env:DALICA_BUILD_DATE); MSI version $MsiVersion"

if (-not $SkipBuild) {
  # Static CRT: no VC++ redistributable needed. Scoped to the target so host build scripts and
  # proc-macros are unaffected.
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  Invoke-Native "cargo build ($Target)" { cargo build --release --locked -p dalica -p dalica-cli --target $Target }
}
$Bin = Join-Path $TargetDir "$Target\release"

# ---- check the programs --------------------------------------------------------------------------
# Machine must match -Arch; dalica.exe is a GUI program (no console window opens with it) and
# dalica-cli.exe a console one (its output reaches the terminal).
foreach ($check in @(@('dalica.exe', 2), @('dalica-cli.exe', 3))) {
  $h = Get-PeHeader (Join-Path $Bin $check[0])
  if ($h.Machine -ne $Machine) { throw "$($check[0]) is for machine 0x$('{0:X}' -f $h.Machine), expected 0x$('{0:X}' -f $Machine) ($Arch)" }
  if ($h.Subsystem -ne $check[1]) { throw "$($check[0]) has PE subsystem $($h.Subsystem), expected $($check[1])" }
  Write-Output "ok $($check[0]): $Arch, PE subsystem $($h.Subsystem)"
}
# Both programs must report this version and the commit, not a dev build. Capturing the output
# also makes PowerShell wait for the GUI program.
foreach ($exe in @('dalica.exe', 'dalica-cli.exe')) {
  $out = (& (Join-Path $Bin $exe) --version | Out-String).Trim()
  if ($LASTEXITCODE -ne 0) { throw "$exe --version failed with exit code $LASTEXITCODE" }
  Write-Output "$exe --version: $out"
  if (-not $out.Contains("$Version (")) { throw "$exe reports '$out', not $Version" }
  if ($ShortSha -and -not $out.Contains($ShortSha)) { throw "$exe does not report commit $ShortSha" }
}
# dalica.exe's version resource (build.rs): what Explorer, the taskbar and Open with show.
$info = (Get-Item (Join-Path $Bin 'dalica.exe')).VersionInfo
if ($info.ProductName -ne 'Dalica' -or $info.ProductVersion -ne $Version) {
  throw "dalica.exe's version resource says '$($info.ProductName) $($info.ProductVersion)', not 'Dalica $Version'"
}
if (-not (Test-EmbeddedIcon (Join-Path $Bin 'dalica.exe') (Join-Path $Root 'assets\app-icon\dalica.ico'))) { throw 'dalica.exe does not carry the app icon' }
Write-Output "ok dalica.exe version resource: $($info.ProductName) $($info.ProductVersion) ($($info.FileVersion)), app icon"

# ---- stage ---------------------------------------------------------------------------------------
$Stage = Join-Path $TargetDir "windows-package\$Arch"
$StageBin = Join-Path $Stage 'bin'
$StageDoc = Join-Path $Stage 'doc'
Remove-Item -Recurse -Force $Stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $StageBin, $StageDoc | Out-Null
Copy-Item (Join-Path $Bin 'dalica.exe'), (Join-Path $Bin 'dalica-cli.exe') $StageBin
# Every package ships the licences, the notice, the asset attributions and the fonts' licences.
foreach ($f in @('LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md', 'README.md')) {
  Copy-Item (Join-Path $Root $f) $StageDoc
}
Copy-Item (Join-Path $Root 'assets\fonts\OFL-Inter.txt') $StageDoc
if ($env:CRAFT_FONTS_DIR) {
  foreach ($lic in Get-ChildItem -Path (Join-Path $env:CRAFT_FONTS_DIR 'fonts\*\OFL.txt') -ErrorAction SilentlyContinue) {
    Copy-Item $lic.FullName (Join-Path $StageDoc "OFL-$($lic.Directory.Name).txt")
  }
}
& (Join-Path $PSScriptRoot 'sign.ps1') (Join-Path $StageBin 'dalica.exe') (Join-Path $StageBin 'dalica-cli.exe')

# ---- MSI -----------------------------------------------------------------------------------------
# WiX v5 as a .NET tool, with the two extensions dalica.wxs uses. Returns whether it is usable.
function Initialize-Wix {
  $tools = Join-Path $env:USERPROFILE '.dotnet\tools'
  $install = $InstallTools -or $InCi
  if (-not (Get-Command wix -ErrorAction SilentlyContinue)) {
    if (-not $install) { return $false }
    Invoke-Native "install WiX $WixVersion" { dotnet tool install --global wix --version $WixVersion } | Write-Host
    if (-not (($env:PATH -split ';') -contains $tools)) { $env:PATH = "$tools;$env:PATH" }
  }
  $have = (wix --version | Out-String).Trim()
  if ($have -notmatch '^5\.') { throw "dalica.wxs needs WiX v5; this machine has wix $have" }
  $exts = (wix extension list -g | Out-String)
  foreach ($ext in 'WixToolset.UI.wixext', 'WixToolset.Util.wixext') {
    if ($exts -notmatch [regex]::Escape($ext)) {
      if (-not $install) { Write-Warning "WiX extension $ext is missing (wix extension add -g $ext/$WixVersion)"; return $false }
      Invoke-Native "add $ext" { wix extension add -g "$ext/$WixVersion" } | Write-Host
    }
  }
  Write-Host "WiX $have"
  return $true
}

# The setup wizard's art, drawn from the app icon so the MSI shows Dalica rather than WiX's stock
# bitmaps: a 493x58 banner across the top of the inner pages (the page title is drawn over its
# left side, so that stays white) and a 493x312 backdrop for the Welcome and Finish pages (their
# text sits on the right; the left 164 px are ours). Both must be 24-bit BMPs. Colours: the icon's
# paper and the Dalica teal (assets/app-icon/README.md).
function New-InstallerArt([string] $IconPng, [string] $BannerOut, [string] $DialogOut) {
  Add-Type -AssemblyName System.Drawing
  $paper = [System.Drawing.ColorTranslator]::FromHtml('#efe9dc')
  $accent = [System.Drawing.ColorTranslator]::FromHtml('#1fa38a')
  $icon = [System.Drawing.Image]::FromFile($IconPng)
  try {
    foreach ($spec in @(@($BannerOut, 493, 58), @($DialogOut, 493, 312))) {
      $bmp = New-Object System.Drawing.Bitmap $spec[1], $spec[2], ([System.Drawing.Imaging.PixelFormat]::Format24bppRgb)
      $g = [System.Drawing.Graphics]::FromImage($bmp)
      try {
        $g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
        $g.Clear([System.Drawing.Color]::White)
        if ($spec[2] -eq 58) {
          $g.DrawImage($icon, 493 - 46 - 8, 6, 46, 46)
        } else {
          $panel = New-Object System.Drawing.SolidBrush $paper
          $stripe = New-Object System.Drawing.SolidBrush $accent
          $g.FillRectangle($panel, 0, 0, 164, 312)
          $g.FillRectangle($stripe, 160, 0, 4, 312)
          $g.DrawImage($icon, 18, 64, 124, 124)
          $panel.Dispose(); $stripe.Dispose()
        }
      } finally { $g.Dispose() }
      $bmp.Save($spec[0], [System.Drawing.Imaging.ImageFormat]::Bmp)
      $bmp.Dispose()
    }
  } finally { $icon.Dispose() }
}

function Build-Msi([string] $Out) {
  Invoke-Native "wix build $(Split-Path -Leaf $Out)" {
    wix build (Join-Path $PSScriptRoot 'dalica.wxs') -arch $Arch `
      -ext WixToolset.UI.wixext -ext WixToolset.Util.wixext `
      -culture en-US -loc (Join-Path $PSScriptRoot 'dalica.en-us.wxl') `
      -d "Version=$MsiVersion" -d "DisplayVersion=$Version" -d "BinDir=$StageBin" -d "DocDir=$StageDoc" `
      -d "IconPath=$(Join-Path $Root 'assets\app-icon\dalica.ico')" -d "BannerBmp=$Banner" -d "DialogBmp=$Dialog" `
      -o $Out
  }
  # wix writes its debug symbols (.wixpdb) next to the MSI; keep them out of the release assets.
  Remove-Item -Force -ErrorAction SilentlyContinue ([IO.Path]::ChangeExtension($Out, '.wixpdb'))
}

$Msi = Join-Path $Dist "dalica-$Version-windows-$Arch.msi"
$UpgradeMsi = Join-Path $Stage 'upgrade-probe.msi'
Remove-Item -Force $Msi -ErrorAction SilentlyContinue
$dotnetTools = Join-Path $env:USERPROFILE '.dotnet\tools'
if ((Test-Path $dotnetTools) -and -not (($env:PATH -split ';') -contains $dotnetTools)) { $env:PATH = "$dotnetTools;$env:PATH" }
$HaveMsi = Initialize-Wix
if ($HaveMsi) {
  $Banner = Join-Path $Stage 'installer-banner.bmp'
  $Dialog = Join-Path $Stage 'installer-dialog.bmp'
  New-InstallerArt (Join-Path $Root 'assets\app-icon\hicolor\256x256\apps\org.dalica.Dalica.png') $Banner $Dialog
  Build-Msi $Msi
  Invoke-Native 'MSI shortcut icon validation (ICE50)' {
    wix msi validate $Msi -ice ICE50 -intermediateFolder (Join-Path $Stage 'msi-validation')
  }
  & (Join-Path $PSScriptRoot 'sign.ps1') $Msi
  # A second build of the same version has another product code: the same-version upgrade test.
  if ($RunTests) { Build-Msi $UpgradeMsi }
  Write-Output "wrote $Msi"
} else {
  if ($InCi) { throw 'WiX could not be set up, so there is no MSI' }
  Write-Warning "MSI skipped: WiX v5 is not installed. Install it with: dotnet tool install --global wix --version $WixVersion; wix extension add -g WixToolset.UI.wixext/$WixVersion WixToolset.Util.wixext/$WixVersion (or run with -InstallTools)."
}

# ---- portable zip --------------------------------------------------------------------------------
$Name = "dalica-$Version-windows-$Arch-portable"
$Portable = Join-Path $Stage $Name
New-Item -ItemType Directory -Force -Path $Portable | Out-Null
Copy-Item (Join-Path $StageBin '*.exe') $Portable
Copy-Item (Join-Path $StageDoc '*') $Portable
# The marker beside the exe keeps the library, the caches and the log in `data` next to it
# (portable mode, apps/dalica/src/paths.rs). Its contents are ignored; it explains itself.
Copy-Item (Join-Path $PSScriptRoot 'portable') (Join-Path $Portable 'portable')
$Zip = Join-Path $Dist "$Name.zip"
Remove-Item -Force $Zip -ErrorAction SilentlyContinue
Compress-Archive -Path $Portable -DestinationPath $Zip
Write-Output "wrote $Zip"
Add-Type -AssemblyName System.IO.Compression.FileSystem
$archive = [System.IO.Compression.ZipFile]::OpenRead($Zip)
try { $archive.Entries | ForEach-Object { Write-Output "  $($_.FullName) ($($_.Length) bytes)" } } finally { $archive.Dispose() }

# ---- tests ---------------------------------------------------------------------------------------
$artifacts = @(Get-Item $Zip) + @(if ($HaveMsi) { Get-Item $Msi })
Add-Summary "#### Windows $Arch packages"
foreach ($a in $artifacts) { Add-Summary "- ``$($a.Name)``: $([Math]::Round($a.Length / 1MB, 2)) MiB ($($a.Length) bytes)" }
if ($RunTests) {
  & (Join-Path $PSScriptRoot 'test-build-rs.ps1') -Version $Version -Target $Target
  Add-Summary '- build.rs: fails naming a missing resource compiler; with rc.exe the exe has its icon and version resource, GUI in release, console in debug'
  & (Join-Path $PSScriptRoot 'test-portable.ps1') -Zip $Zip
  Add-Summary '- portable zip: library and log in `data\` beside dalica.exe, nothing in LOCALAPPDATA or APPDATA; without the marker the platform folder is used'
  if ($HaveMsi) {
    & (Join-Path $PSScriptRoot 'test-msi.ps1') -Msi $Msi -UpgradeMsi $UpgradeMsi -Version $Version
    Add-Summary "- MSI: msiexec /qn installs and uninstalls cleanly; installed dalica.exe is Dalica $Version (GUI), both --version match; Open with listed, defaults untouched; same-version upgrade replaces"
  }
} else {
  Write-Output 'tests skipped (they run in CI, or with -Test)'
}
$artifacts | Format-Table Name, Length
