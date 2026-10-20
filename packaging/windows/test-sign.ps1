<#
.SYNOPSIS
  Check the Authenticode signature of each Windows artifact: signed, by the expected certificate,
  with a timestamp. Refuses an unsigned file when signing was asked; otherwise names each one.

.DESCRIPTION
  For every file (an .exe, or an .msi together with the programs inside it, unpacked with an
  administrative install `msiexec /a`), Get-AuthenticodeSignature must report:
    - a signature whose status is Valid (the chain ends in a root this machine trusts),
    - the expected signer certificate: -Thumbprint when given; else the certificate in
      WINDOWS_CERTIFICATE (base64 .pfx, opened with WINDOWS_CERTIFICATE_PASSWORD); with Azure
      Trusted Signing (whose certificates are renewed daily) one and the same signer for every file,
    - an RFC 3161 timestamp (a timestamping certificate on the signature).

  Signing counts as asked when -Thumbprint is given or the signing material sign.ps1 reads is set
  (WINDOWS_CERTIFICATE + WINDOWS_CERTIFICATE_PASSWORD, or the six AZURE_* variables): then an
  unsigned file or a failed check is an error. Without them each unsigned file is named in a
  warning and the script succeeds, as sign.ps1 leaves them. Each file's state goes to the job
  summary (GITHUB_STEP_SUMMARY) when there is one. Secrets reach the script only through the
  environment.

  Dalica's own (M15.10); package.ps1 runs it after signing.

.EXAMPLE
  pwsh packaging/windows/test-sign.ps1 target/windows-package/x64/bin/dalica.exe dist/release/dalica-0.1.0-windows-x64.msi
#>
# Only -Thumbprint by name: every positional argument is a file (package.ps1 splats the paths).
[CmdletBinding(PositionalBinding = $false)]
param(
  [Parameter(Mandatory = $true, ValueFromRemainingArguments = $true)]
  [string[]] $Files,
  [Parameter()]
  [string] $Thumbprint
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Write-Warn([string] $Message) {
  if ($env:GITHUB_ACTIONS) { Write-Output "::warning::$Message" } else { Write-Warning $Message }
}
function Add-Summary([string] $Line) {
  if ($env:GITHUB_STEP_SUMMARY) { Add-Content -Path $env:GITHUB_STEP_SUMMARY -Value $Line -Encoding utf8 }
}

$haveCert = [bool]$env:WINDOWS_CERTIFICATE -and [bool]$env:WINDOWS_CERTIFICATE_PASSWORD
$azureVars = 'AZURE_TENANT_ID', 'AZURE_CLIENT_ID', 'AZURE_CLIENT_SECRET', 'AZURE_SIGNING_ENDPOINT', 'AZURE_SIGNING_ACCOUNT', 'AZURE_CERT_PROFILE'
$haveAzure = -not ($azureVars | Where-Object { -not [Environment]::GetEnvironmentVariable($_) })
$Asked = [bool]$Thumbprint -or $haveCert -or $haveAzure

# The expected signer: -Thumbprint, else the .pfx's code-signing certificate (the one with a key).
$Expected = if ($Thumbprint) { $Thumbprint.ToUpperInvariant() -replace '[^0-9A-F]', '' } else { '' }
if (-not $Expected -and $haveCert) {
  $certs = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2Collection
  $certs.Import([Convert]::FromBase64String($env:WINDOWS_CERTIFICATE), $env:WINDOWS_CERTIFICATE_PASSWORD,
    [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
  $signer = @($certs | Where-Object { $_.HasPrivateKey }) | Select-Object -First 1
  if (-not $signer) { throw 'WINDOWS_CERTIFICATE holds no certificate with a private key' }
  $Expected = $signer.Thumbprint.ToUpperInvariant()
}

# Each file to check, with the programs an MSI carries (so the MSI is known to wrap signed programs).
$tmp = Join-Path ([IO.Path]::GetTempPath()) "dalica-test-sign-$([Guid]::NewGuid().ToString('N'))"
$targets = New-Object System.Collections.Generic.List[object]
try {
  foreach ($f in $Files) {
    $path = (Resolve-Path $f).Path
    $targets.Add([pscustomobject]@{ Path = $path; Name = Split-Path -Leaf $path })
    if ($path -match '\.msi$') {
      $out = Join-Path $tmp ([IO.Path]::GetFileNameWithoutExtension($path))
      New-Item -ItemType Directory -Force -Path $out | Out-Null
      $log = Join-Path $tmp "$([IO.Path]::GetFileNameWithoutExtension($path)).log"
      $p = Start-Process msiexec.exe -ArgumentList @('/a', "`"$path`"", '/qn', "TARGETDIR=`"$out`"", '/l*v', "`"$log`"") -Wait -PassThru
      if ($p.ExitCode -ne 0) {
        if (Test-Path $log) { Get-Content $log -Tail 40 }
        throw "msiexec /a $(Split-Path -Leaf $path) exited $($p.ExitCode)"
      }
      $inside = @(Get-ChildItem -Path $out -Recurse -Filter *.exe)
      if ($inside.Count -eq 0) { throw "$(Split-Path -Leaf $path) holds no programs" }
      foreach ($exe in $inside) {
        $targets.Add([pscustomobject]@{ Path = $exe.FullName; Name = "$(Split-Path -Leaf $path) > $($exe.Name)" })
      }
    }
  }

  $unsigned = @()
  $failures = @()
  $signers = @{}
  Add-Summary '#### Windows signatures'
  foreach ($t in $targets) {
    $sig = Get-AuthenticodeSignature -FilePath $t.Path
    if (-not $sig.SignerCertificate) {
      $unsigned += $t.Name
      Add-Summary "- ``$($t.Name)``: unsigned"
      continue
    }
    $thumb = $sig.SignerCertificate.Thumbprint.ToUpperInvariant()
    $signers[$thumb] = $sig.SignerCertificate.Subject
    $problems = @()
    if ($sig.Status -ne 'Valid') { $problems += "status $($sig.Status): $($sig.StatusMessage)" }
    if ($Expected -and $thumb -ne $Expected) { $problems += "signed by $thumb ($($sig.SignerCertificate.Subject)), expected $Expected" }
    if (-not $sig.TimeStamperCertificate) { $problems += 'no timestamp' }
    $stamp = if ($sig.TimeStamperCertificate) { "timestamped by $($sig.TimeStamperCertificate.Subject)" } else { 'no timestamp' }
    if ($problems) {
      $failures += "$($t.Name): $($problems -join '; ')"
      Add-Summary "- ``$($t.Name)``: **check failed**: $($problems -join '; ')"
    } else {
      Write-Output "ok $($t.Name): signed by $thumb ($($sig.SignerCertificate.Subject)), $stamp"
      Add-Summary "- ``$($t.Name)``: signed by ``$thumb`` ($($sig.SignerCertificate.Subject)), $stamp"
    }
  }
  # Without a pinned certificate (Azure Trusted Signing), every file must still share one signer.
  if (-not $Expected -and $signers.Count -gt 1) {
    $failures += "the files are signed by $($signers.Count) different certificates: $(($signers.Keys | Sort-Object) -join ', ')"
  }

  if ($Asked) {
    foreach ($u in $unsigned) { $failures += "${u}: unsigned, but signing was asked" }
    if ($failures) {
      $failures | ForEach-Object { Write-Output "::error::$_" }
      throw "$($failures.Count) signature check(s) failed"
    }
    Write-Output "ok: $($targets.Count) file(s) signed$(if ($Expected) { " by $Expected" }) with a timestamp"
  } else {
    foreach ($u in $unsigned) { Write-Warn "${u} is unsigned (no Windows signing secrets)" }
    if ($failures) {
      $failures | ForEach-Object { Write-Output "::error::$_" }
      throw "$($failures.Count) signature check(s) failed"
    }
  }
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
