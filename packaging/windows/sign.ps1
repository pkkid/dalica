<#
.SYNOPSIS
  Authenticode-sign files with signtool, using whichever signing material is configured.

.DESCRIPTION
  Tries, in order:
    1. A code-signing certificate file:   WINDOWS_CERTIFICATE (base64 .pfx), WINDOWS_CERTIFICATE_PASSWORD
    2. Azure Trusted Signing:             AZURE_TENANT_ID, AZURE_CLIENT_ID, AZURE_CLIENT_SECRET,
                                          AZURE_SIGNING_ENDPOINT (e.g. https://eus.codesigning.azure.net),
                                          AZURE_SIGNING_ACCOUNT, AZURE_CERT_PROFILE
  With neither, it prints a warning naming each file it leaves unsigned (exit code 0), so test
  builds still produce installers. Secrets reach the script only through these environment
  variables, never as arguments.

  Every signature is SHA-256 with an RFC 3161 timestamp, and is verified (signtool verify /pa)
  before the script returns. A timestamp server that does not answer is retried (three tries, 10 s
  apart) before the signing fails. This is the one place to adapt when the signing setup changes
  (an HSM/cloud key, a different timestamp server, ...). Optional overrides: WINDOWS_TIMESTAMP_URL,
  SIGNTOOL (path to signtool.exe). docs/releasing.md -> Secrets lists the secrets; the release
  workflow's `sign Windows x64` job runs this through package.ps1 -FromStage, and test-sign.ps1
  checks the result.

  -Throwaway (CI only) makes the release dry run's signing material instead of signing anything:
  a self-signed code-signing certificate valid for one day, trusted on this runner only (its
  LocalMachine Root store, so `signtool verify /pa` and the checks pass on the same path a real
  certificate takes), exported as a .pfx with a random password into WINDOWS_CERTIFICATE and
  WINDOWS_CERTIFICATE_PASSWORD of the calling PowerShell process. It is never trusted anywhere
  else and never reaches a draft. It refuses to run outside GitHub Actions, because it changes the
  machine's trusted roots.

  Adapted from LightCraft packaging/windows/sign.ps1 (revision 453f97b5, unchanged at d999e382),
  identical to PhotoCraft's (revision 1f1ddb2a, unchanged at 3a896a5e); MIT OR Apache-2.0. Changed
  for Dalica: the names, a certificate needs its password too, the warning names each file, the
  timestamp retries and the -Throwaway dry-run certificate.

.EXAMPLE
  pwsh packaging/windows/sign.ps1 target/windows-package/x64/bin/dalica.exe dist/release/dalica-0.1.0-windows-x64.msi
.EXAMPLE
  ./packaging/windows/sign.ps1 -Throwaway; ./packaging/windows/package.ps1 -Arch x64 -FromStage stage
#>
[CmdletBinding(DefaultParameterSetName = 'Sign')]
param(
  [Parameter(ParameterSetName = 'Sign', Mandatory = $true, ValueFromRemainingArguments = $true)]
  [string[]] $Files,
  [Parameter(ParameterSetName = 'Throwaway', Mandatory = $true)]
  [switch] $Throwaway
)
$ErrorActionPreference = 'Stop'

function Write-Warn([string] $Message) {
  if ($env:GITHUB_ACTIONS) { Write-Output "::warning::$Message" } else { Write-Warning $Message }
}

if ($Throwaway) {
  if ($env:GITHUB_ACTIONS -ne 'true') { throw '-Throwaway trusts a new root certificate on this machine; it runs on CI runners only' }
  $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=Dalica dry run (throwaway, untrusted)' `
    -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 -KeyExportPolicy Exportable `
    -CertStoreLocation Cert:\CurrentUser\My -NotAfter (Get-Date).AddDays(1)
  $tmp = Join-Path ([IO.Path]::GetTempPath()) "dalica-throwaway-$PID"
  New-Item -ItemType Directory -Force -Path $tmp | Out-Null
  try {
    $password = [Guid]::NewGuid().ToString('N') + [Guid]::NewGuid().ToString('N')
    Write-Output "::add-mask::$password"
    $pfx = Join-Path $tmp 'throwaway.pfx'
    $cer = Join-Path $tmp 'throwaway.cer'
    Export-PfxCertificate -Cert $cert -FilePath $pfx -Password (ConvertTo-SecureString $password -AsPlainText -Force) | Out-Null
    Export-Certificate -Cert $cert -FilePath $cer | Out-Null
    # Trusted on this runner only, so verification takes the same path as a real certificate.
    Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\Root | Out-Null
    $env:WINDOWS_CERTIFICATE = [Convert]::ToBase64String([IO.File]::ReadAllBytes($pfx))
    $env:WINDOWS_CERTIFICATE_PASSWORD = $password
    # A throwaway run never mixes with Azure material.
    foreach ($v in 'AZURE_TENANT_ID', 'AZURE_CLIENT_ID', 'AZURE_CLIENT_SECRET', 'AZURE_SIGNING_ENDPOINT', 'AZURE_SIGNING_ACCOUNT', 'AZURE_CERT_PROFILE') {
      [Environment]::SetEnvironmentVariable($v, $null)
    }
    Write-Output "Throwaway signing certificate $($cert.Thumbprint) ($($cert.Subject)), valid until $($cert.NotAfter.ToUniversalTime().ToString('u')), trusted on this runner only"
  }
  finally {
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
  }
  return
}

function Find-SignTool {
  if ($env:SIGNTOOL) { return $env:SIGNTOOL }
  $onPath = Get-Command signtool.exe -ErrorAction SilentlyContinue
  if ($onPath) { return $onPath.Source }
  $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
  $found = Get-ChildItem -Path $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '\\x64\\signtool\.exe$' } |
    Sort-Object FullName -Descending | Select-Object -First 1
  if (-not $found) { throw 'signtool.exe not found (install the Windows SDK)' }
  return $found.FullName
}

function Invoke-SignTool([string[]] $Arguments, [int] $Tries = 1) {
  for ($try = 1; ; $try++) {
    & $script:SignTool @Arguments
    if ($LASTEXITCODE -eq 0) { return }
    if ($try -ge $Tries) { throw "signtool $($Arguments[0]) failed with exit code $LASTEXITCODE" }
    Write-Warn "signtool $($Arguments[0]) failed with exit code $LASTEXITCODE (try $try of $Tries; the timestamp server may be busy); retrying in 10 s"
    Start-Sleep -Seconds 10
  }
}

$Files = $Files | ForEach-Object { (Resolve-Path $_).Path }
$haveCert = [bool]$env:WINDOWS_CERTIFICATE -and [bool]$env:WINDOWS_CERTIFICATE_PASSWORD
$azureVars = 'AZURE_TENANT_ID', 'AZURE_CLIENT_ID', 'AZURE_CLIENT_SECRET', 'AZURE_SIGNING_ENDPOINT', 'AZURE_SIGNING_ACCOUNT', 'AZURE_CERT_PROFILE'
$haveAzure = -not ($azureVars | Where-Object { -not [Environment]::GetEnvironmentVariable($_) })

if (-not $haveCert -and -not $haveAzure) {
  foreach ($f in $Files) {
    Write-Warn "No Windows signing secrets (WINDOWS_CERTIFICATE + WINDOWS_CERTIFICATE_PASSWORD, or the six AZURE_* secrets): leaving $(Split-Path -Leaf $f) unsigned"
  }
  exit 0
}

$script:SignTool = Find-SignTool
$common = @('sign', '/v', '/fd', 'SHA256', '/td', 'SHA256', '/d', 'Dalica', '/du', 'https://github.com/pkkid/dalica')
$tmp = Join-Path ([IO.Path]::GetTempPath()) "dalica-sign-$PID"
New-Item -ItemType Directory -Force -Path $tmp | Out-Null

try {
  if ($haveCert) {
    Write-Output "Signing with WINDOWS_CERTIFICATE: $($Files -join ', ')"
    $pfx = Join-Path $tmp 'cert.pfx'
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    $ts = if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { 'http://timestamp.digicert.com' }
    $signArgs = $common + @('/tr', $ts, '/f', $pfx)
    $signArgs += @('/p', $env:WINDOWS_CERTIFICATE_PASSWORD)
    Invoke-SignTool ($signArgs + $Files) -Tries 3
  }
  else {
    Write-Output "Signing with Azure Trusted Signing ($env:AZURE_SIGNING_ACCOUNT / $env:AZURE_CERT_PROFILE): $($Files -join ', ')"
    # The dlib authenticates with DefaultAzureCredential, which reads AZURE_TENANT_ID,
    # AZURE_CLIENT_ID and AZURE_CLIENT_SECRET from the environment.
    $pkg = Join-Path $tmp 'trusted-signing.zip'
    Invoke-WebRequest -Uri 'https://www.nuget.org/api/v2/package/Microsoft.Trusted.Signing.Client' -OutFile $pkg
    Expand-Archive -Path $pkg -DestinationPath (Join-Path $tmp 'client') -Force
    $dlib = Get-ChildItem -Path (Join-Path $tmp 'client') -Recurse -Filter Azure.CodeSigning.Dlib.dll |
      Where-Object { $_.FullName -match '\\x64\\' } | Select-Object -First 1
    if (-not $dlib) { throw 'Azure.CodeSigning.Dlib.dll not found in Microsoft.Trusted.Signing.Client' }
    $metadata = Join-Path $tmp 'metadata.json'
    @{
      Endpoint               = $env:AZURE_SIGNING_ENDPOINT
      CodeSigningAccountName = $env:AZURE_SIGNING_ACCOUNT
      CertificateProfileName = $env:AZURE_CERT_PROFILE
    } | ConvertTo-Json | Set-Content -Path $metadata -Encoding utf8
    $ts = if ($env:WINDOWS_TIMESTAMP_URL) { $env:WINDOWS_TIMESTAMP_URL } else { 'http://timestamp.acs.microsoft.com' }
    Invoke-SignTool ($common + @('/tr', $ts, '/dlib', $dlib.FullName, '/dmdf', $metadata) + $Files) -Tries 3
  }
  Invoke-SignTool (@('verify', '/pa', '/v') + $Files)
}
finally {
  Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
}
