<#
.SYNOPSIS
  Check that the portable zip keeps Dalica's data beside the executable.

.DESCRIPTION
  Unpacks the zip twice into a temporary folder and starts the unpacked dalica.exe headless
  (`--headless --control <free port>`, token file in the temporary folder) with LOCALAPPDATA and
  APPDATA pointed at empty folders of their own:
    1. as shipped (with the `portable` marker): the library appears in `data\library` beside the
       exe, the thumbnail cache in `data\cache\thumbs` and the log in `data\logs`, and nothing is
       written under LOCALAPPDATA or APPDATA;
    2. with the marker deleted (the control case): the library appears in
       LOCALAPPDATA\Dalica\Library and no `data` folder is made, so case 1 passed because of the
       marker and not by accident.
  Each instance is stopped once its library folder has files. Runs anywhere Windows can start the
  app headless (no GPU needed); package.ps1 runs it in CI.

.EXAMPLE
  pwsh packaging/windows/test-portable.ps1 -Zip dist/release/dalica-0.1.0-windows-x64-portable.zip
#>
param(
  [Parameter(Mandatory = $true)] [string] $Zip,
  [int] $TimeoutSeconds = 120
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$Zip = (Resolve-Path $Zip).Path
$tmp = Join-Path ([IO.Path]::GetTempPath()) "dalica-portable-test-$([Guid]::NewGuid().ToString('N'))"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

function Get-FreePort {
  $l = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
  $l.Start()
  try { return $l.LocalEndpoint.Port } finally { $l.Stop() }
}

# Unpacks the zip into $tmp\$Name and returns the folder that holds dalica.exe.
function Expand-Portable([string] $Name) {
  $dest = Join-Path $tmp $Name
  Expand-Archive -Path $Zip -DestinationPath $dest
  $exe = @(Get-ChildItem -Path $dest -Recurse -Filter dalica.exe)
  if ($exe.Count -ne 1) { throw "the zip holds $($exe.Count) dalica.exe files, expected 1" }
  return $exe[0].DirectoryName
}

# Starts dalica.exe in $Dir headless with its own empty user folders, waits until the library folder
# (beside the exe with -Portable, else under LOCALAPPDATA) holds a file, stops it, and returns the
# user folders it was given.
function Invoke-Headless([string] $Name, [string] $Dir, [switch] $Portable) {
  $user = Join-Path $tmp "$Name-user"
  $local = Join-Path $user 'LocalAppData'
  $roaming = Join-Path $user 'AppData'
  New-Item -ItemType Directory -Force -Path $local, $roaming, (Join-Path $user 'Temp') | Out-Null
  $psi = [System.Diagnostics.ProcessStartInfo]::new((Join-Path $Dir 'dalica.exe'))
  foreach ($a in @('--headless', '--control', [string](Get-FreePort), '--control-token-file', (Join-Path $user 'control.token'))) {
    $psi.ArgumentList.Add($a)
  }
  $psi.UseShellExecute = $false
  $psi.WorkingDirectory = $Dir
  $psi.RedirectStandardError = $true
  $psi.RedirectStandardOutput = $true
  $psi.Environment['LOCALAPPDATA'] = $local
  $psi.Environment['APPDATA'] = $roaming
  $psi.Environment['TEMP'] = Join-Path $user 'Temp'
  $psi.Environment['TMP'] = Join-Path $user 'Temp'
  foreach ($k in 'DALICA_HOME', 'DALICA_CACHE_DIR', 'DALICA_CONTROL_PORT', 'DALICA_CONTROL_TOKEN_FILE') { [void]$psi.Environment.Remove($k) }
  $p = [System.Diagnostics.Process]::Start($psi)
  # Read both pipes in the background so a full pipe never stalls the app.
  $errTask = $p.StandardError.ReadToEndAsync()
  $outTask = $p.StandardOutput.ReadToEndAsync()
  $lib = if ($Portable) { Join-Path $Dir 'data\library' } else { Join-Path $local 'Dalica\Library' }
  try {
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)
    while ($true) {
      $ready = (Test-Path $lib) -and @(Get-ChildItem -Force $lib).Count -gt 0
      # In portable mode also wait for the thumbnail cache and the log beside the exe.
      if ($Portable) { $ready = $ready -and (Test-Path (Join-Path $Dir 'data\cache\thumbs')) -and (Test-Path (Join-Path $Dir 'data\logs')) }
      if ($ready) { break }
      if ($p.HasExited) { throw "$Name`: dalica exited with $($p.ExitCode) before making $lib`n$($errTask.Result)" }
      if ((Get-Date) -gt $deadline) { throw "$Name`: no library in $lib after $TimeoutSeconds s" }
      Start-Sleep -Milliseconds 250
    }
    Write-Host "ok $Name`: the library is in $lib ($(@(Get-ChildItem -Force $lib).Name -join ', '))"
  } finally {
    if (-not $p.HasExited) { $p.Kill($true); $p.WaitForExit() }
    [void]$errTask.Wait(5000)
    [void]$outTask.Wait(5000)
    $p.Dispose()
  }
  return @{ Local = $local; Roaming = $roaming }
}

$passed = $false
try {
  # ---- 1. as shipped: everything beside the exe -------------------------------------------------
  $dir = Expand-Portable 'shipped'
  if (-not (Test-Path -PathType Leaf (Join-Path $dir 'portable'))) { throw 'the zip has no portable marker beside dalica.exe' }
  $user = Invoke-Headless 'portable' $dir -Portable
  foreach ($sub in 'data\logs', 'data\cache\thumbs') {
    if (-not (Test-Path (Join-Path $dir $sub))) { throw "portable mode: no $sub beside the exe" }
  }
  foreach ($folder in $user.Local, $user.Roaming) {
    $written = @(Get-ChildItem -Force -Recurse $folder)
    if ($written.Count -ne 0) { throw "portable mode wrote outside its folder: $($written.FullName -join ', ')" }
  }
  Write-Output 'ok portable: library, thumbnail cache and log in data\ beside dalica.exe; LOCALAPPDATA and APPDATA untouched'

  # ---- 2. control: without the marker the platform folder is used ------------------------------
  $dir = Expand-Portable 'unmarked'
  Remove-Item -Force (Join-Path $dir 'portable')
  $null = Invoke-Headless 'unmarked' $dir
  if (Test-Path (Join-Path $dir 'data')) { throw 'without the marker a data folder was still made beside the exe' }
  Write-Output 'ok unmarked: the library goes to LOCALAPPDATA\Dalica\Library and no data folder is made'
  $passed = $true
} finally {
  if ($passed) { Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue } else { Write-Output "test files kept in $tmp" }
}
