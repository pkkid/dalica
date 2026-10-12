<#
  Get-PeHeader: the machine (COFF header) and subsystem (optional header) of a Windows executable.
  Machine 0x8664 is x64, 0xAA64 ARM64; subsystem 2 is a Windows GUI program, 3 a console program.
  Test-EmbeddedIcon: whether an executable carries every image of an .ico file (the resource
  compiler stores them byte for byte as RT_ICON resources).
  Dot-sourced by package.ps1 and the tests beside it.

  Adapted from PhotoCraft packaging/windows/package.ps1 (revision 0d6e658e); MIT OR Apache-2.0.
  Changed for Dalica: bounds checks, so a truncated or foreign file is an error, not an exception
  deep in BitConverter.
#>
function Get-PeHeader([string] $Path) {
  $bytes = [System.IO.File]::ReadAllBytes($Path)
  if ($bytes.Length -lt 0x40 -or $bytes[0] -ne 0x4D -or $bytes[1] -ne 0x5A) { throw "$Path is not a Windows executable (no MZ header)" }
  $pe = [BitConverter]::ToInt32($bytes, 0x3C)
  if ($pe -lt 0 -or $pe + 0x5E -gt $bytes.Length) { throw "$Path has no PE header where its MZ header points" }
  if ([BitConverter]::ToUInt32($bytes, $pe) -ne 0x00004550) { throw "$Path has no PE signature" }
  return @{ Machine = [BitConverter]::ToUInt16($bytes, $pe + 4); Subsystem = [BitConverter]::ToUInt16($bytes, $pe + 0x5C) }
}

function Test-EmbeddedIcon([string] $Exe, [string] $Ico) {
  $ico = [System.IO.File]::ReadAllBytes($Ico)
  if ($ico.Length -lt 6 -or [BitConverter]::ToUInt16($ico, 0) -ne 0 -or [BitConverter]::ToUInt16($ico, 2) -ne 1) { throw "$Ico is not an .ico file" }
  $count = [BitConverter]::ToUInt16($ico, 4)
  if ($count -eq 0 -or 6 + 16 * $count -gt $ico.Length) { throw "$Ico has a damaged directory" }
  # Latin-1 maps every byte to one character, so a byte search becomes an ordinal string search.
  $latin1 = [System.Text.Encoding]::Latin1
  $haystack = $latin1.GetString([System.IO.File]::ReadAllBytes($Exe))
  for ($i = 0; $i -lt $count; $i++) {
    $size = [BitConverter]::ToUInt32($ico, 6 + 16 * $i + 8)
    $offset = [BitConverter]::ToUInt32($ico, 6 + 16 * $i + 12)
    if ($size -eq 0 -or $offset + $size -gt $ico.Length) { throw "$Ico image $i lies outside the file" }
    if (-not $haystack.Contains($latin1.GetString($ico, $offset, $size), [System.StringComparison]::Ordinal)) { return $false }
  }
  return $true
}
