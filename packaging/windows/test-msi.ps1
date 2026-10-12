<#
.SYNOPSIS
  Install the Dalica MSI silently, check what it installed, upgrade it with a second build of the
  same version, and uninstall it cleanly. Needs Windows and administrator rights (CI runners have
  them); it installs into Program Files, so package.ps1 runs it in CI only (or with -Test).

.DESCRIPTION
  1. `msiexec /i <Msi> /qn`: both programs, the licence files, the Start Menu shortcut, the
     uninstall entry with the numeric version; the installed dalica.exe's version resource says
     Dalica and -Version (numeric X.Y.Z.0), it is a GUI program, and both programs' `--version`
     report -Version; Dalica is under "Open with" for .jpg and .cr3 without becoming their default.
  2. `msiexec /i <UpgradeMsi> /qn` (another build of the same version, so another product code):
     it replaces the first, leaving one Dalica installed (same-version upgrades).
  3. `msiexec /x <UpgradeMsi> /qn`: the programs, the shortcut, the uninstall entry and every
     registry value it wrote are gone.
  Every msiexec run writes a verbose log; on failure the script prints its tail.

.EXAMPLE
  pwsh packaging/windows/test-msi.ps1 -Msi dist/release/dalica-0.1.0-windows-x64.msi -UpgradeMsi target/windows-package/x64/upgrade-probe.msi -Version 0.1.0
#>
param(
  [Parameter(Mandatory = $true)] [string] $Msi,
  [Parameter(Mandatory = $true)] [string] $UpgradeMsi,
  [Parameter(Mandatory = $true)] [string] $Version
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'pe.ps1')
$Msi = (Resolve-Path $Msi).Path
$UpgradeMsi = (Resolve-Path $UpgradeMsi).Path
$WantVersion = ($Version -split '-')[0]
$Logs = Join-Path ([IO.Path]::GetTempPath()) "dalica-msi-test-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force -Path $Logs | Out-Null

$Dir = Join-Path $env:ProgramFiles 'Dalica'
$Shortcut = Join-Path $env:ProgramData 'Microsoft\Windows\Start Menu\Programs\Dalica.lnk'
$Exe = Join-Path $Dir 'dalica.exe'
$Cli = Join-Path $Dir 'dalica-cli.exe'

function Invoke-Msi([string] $Name, [string[]] $Arguments) {
  $log = Join-Path $Logs "$Name.log"
  $p = Start-Process msiexec.exe -ArgumentList ($Arguments + @('/qn', '/norestart', '/l*v', "`"$log`"")) -Wait -PassThru
  if ($p.ExitCode -ne 0) {
    if (Test-Path $log) { Get-Content $log -Tail 60 }
    throw "msiexec $Name exited $($p.ExitCode) (log: $log)"
  }
  Write-Output "ok msiexec $Name"
}

# The uninstall entries named Dalica (64- and 32-bit views).
function Get-Installed {
  $roots = 'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall', 'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
  @(foreach ($root in $roots) {
      Get-ChildItem $root -ErrorAction SilentlyContinue | Get-ItemProperty -ErrorAction SilentlyContinue |
        Where-Object { $_.PSObject.Properties['DisplayName'] -and $_.DisplayName -eq 'Dalica' }
    })
}

function Get-RegValue([string] $Key, [string] $Name) {
  $item = Get-ItemProperty -LiteralPath $Key -ErrorAction SilentlyContinue
  if ($item -and $item.PSObject.Properties[$Name]) { return $item.$Name }
  return $null
}

function Test-OpenWith([string] $Ext) {
  return $null -ne (Get-RegValue "HKLM:\Software\Classes\.$Ext\OpenWithProgids" 'Dalica.Image')
}

# Runs `<exe> --version` with its output captured (which also makes PowerShell wait for a GUI
# program) and checks it reports this version.
function Test-Version([string] $Path) {
  $out = (& $Path --version | Out-String).Trim()
  if ($LASTEXITCODE -ne 0) { throw "$Path --version exited $LASTEXITCODE" }
  if (-not $out.Contains("$Version (")) { throw "$Path --version says '$out', not $Version" }
  Write-Output "ok $(Split-Path -Leaf $Path) --version: $out"
}

if ((Get-Installed).Count -ne 0) { throw 'Dalica is already installed on this machine; uninstall it before this test' }
# The defaults of two extensions, which the MSI must not change.
$defaults = @{}
foreach ($ext in 'jpg', 'cr3') { $defaults[$ext] = Get-RegValue "HKLM:\Software\Classes\.$ext" '(default)' }

$installed = $false
$passed = $false
try {
  # ---- 1. install ------------------------------------------------------------------------------
  $installed = $true
  Invoke-Msi 'install' @('/i', "`"$Msi`"")
  foreach ($f in 'dalica.exe', 'dalica-cli.exe', 'LICENSE-MIT', 'LICENSE-APACHE', 'NOTICE', 'ATTRIBUTION.md') {
    if (-not (Test-Path (Join-Path $Dir $f))) { throw "the MSI did not install $f into $Dir" }
  }
  if (-not (Test-Path $Shortcut)) { throw "no Start Menu shortcut at $Shortcut" }
  $entries = Get-Installed
  if ($entries.Count -ne 1) { throw "$($entries.Count) uninstall entries named Dalica after installing, expected 1" }
  if ($entries[0].DisplayVersion -ne $WantVersion) { throw "the uninstall entry says version $($entries[0].DisplayVersion), not $WantVersion" }
  $first = $entries[0].PSChildName

  $info = (Get-Item $Exe).VersionInfo
  if ($info.ProductName -ne 'Dalica') { throw "the installed dalica.exe's ProductName is '$($info.ProductName)'" }
  if ($info.ProductVersion -ne $Version) { throw "the installed dalica.exe's ProductVersion is '$($info.ProductVersion)', not $Version" }
  $fileVersion = "$($info.FileMajorPart).$($info.FileMinorPart).$($info.FileBuildPart)"
  if ($fileVersion -ne $WantVersion) { throw "the installed dalica.exe's file version is $fileVersion, not $WantVersion" }
  if ((Get-PeHeader $Exe).Subsystem -ne 2) { throw 'the installed dalica.exe is not a GUI program' }
  Write-Output "ok installed dalica.exe: $($info.ProductName) $($info.ProductVersion) ($fileVersion.$($info.FilePrivatePart)), GUI"
  Test-Version $Exe
  Test-Version $Cli

  foreach ($ext in 'jpg', 'cr3') {
    if (-not (Test-OpenWith $ext)) { throw "Dalica is not under Open with for .$ext" }
    $now = Get-RegValue "HKLM:\Software\Classes\.$ext" '(default)'
    if ($now -ne $defaults[$ext]) { throw ".$ext's default program changed from '$($defaults[$ext])' to '$now'" }
  }
  $cmd = Get-RegValue 'HKLM:\Software\Classes\Dalica.Image\shell\open\command' '(default)'
  if (-not $cmd -or -not $cmd.Contains($Exe)) { throw "the Open with command is '$cmd'" }
  if ($null -eq (Get-RegValue 'HKLM:\Software\Classes\Applications\dalica.exe\SupportedTypes' '.jpg')) { throw 'dalica.exe does not list .jpg among its supported types' }
  if ((Get-RegValue 'HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths\dalica.exe' '(default)') -ne $Exe) { throw 'the App Paths entry does not point at dalica.exe' }
  Write-Output "ok Open with: .jpg and .cr3 list Dalica, their defaults unchanged; command $cmd"

  # ---- 2. same-version upgrade -----------------------------------------------------------------
  Invoke-Msi 'upgrade' @('/i', "`"$UpgradeMsi`"")
  $entries = Get-Installed
  if ($entries.Count -ne 1) { throw "$($entries.Count) uninstall entries named Dalica after a same-version upgrade, expected 1" }
  if ($entries[0].PSChildName -eq $first) { throw 'the same-version upgrade left the first product installed' }
  if (-not (Test-Path $Exe)) { throw 'dalica.exe is gone after the same-version upgrade' }
  Write-Output "ok same-version upgrade: $first replaced by $($entries[0].PSChildName)"

  # ---- 3. uninstall ----------------------------------------------------------------------------
  Invoke-Msi 'uninstall' @('/x', "`"$UpgradeMsi`"")
  $installed = $false
  if ((Get-Installed).Count -ne 0) { throw 'an uninstall entry named Dalica is left after uninstalling' }
  foreach ($f in $Exe, $Cli, (Join-Path $Dir 'NOTICE'), $Shortcut) {
    if (Test-Path $f) { throw "$f is left after uninstalling" }
  }
  if (Test-Path $Dir) {
    $left = @(Get-ChildItem -Force -Recurse $Dir)
    if ($left.Count -ne 0) { throw "files are left in $Dir after uninstalling: $($left.FullName -join ', ')" }
  }
  foreach ($ext in 'jpg', 'cr3') {
    if (Test-OpenWith $ext) { throw "Dalica is still under Open with for .$ext after uninstalling" }
  }
  foreach ($key in 'HKLM:\Software\Classes\Dalica.Image', 'HKLM:\Software\Classes\Applications\dalica.exe',
    'HKLM:\Software\Microsoft\Windows\CurrentVersion\App Paths\dalica.exe', 'HKLM:\Software\Dalica') {
    if (Test-Path $key) { throw "$key is left after uninstalling" }
  }
  Write-Output 'ok uninstall: programs, shortcut, uninstall entry and registry values removed'
  $passed = $true
} finally {
  if ($installed) {
    foreach ($e in Get-Installed) {
      Start-Process msiexec.exe -ArgumentList @('/x', $e.PSChildName, '/qn', '/norestart') -Wait | Out-Null
    }
  }
  if ($passed) { Remove-Item -Recurse -Force $Logs -ErrorAction SilentlyContinue } else { Write-Output "msiexec logs kept in $Logs" }
}
