param(
  [Parameter(Mandatory = $true)][string]$SdkRoot,
  [Parameter(Mandatory = $true)][string]$OutputPath
)

$sourcePath = Join-Path $SdkRoot 'Examples\Effect\GLator\GL_base.cpp'
$expectedHash = '3D1FCD345A1DF48C507F2401573AFCB113B1851314008341953E66A0BFFF57F6'
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $sourcePath).Hash -ne $expectedHash) {
  throw 'GL_base.cpp does not match the reviewed SDK source.'
}

$source = [IO.File]::ReadAllText($sourcePath)
$replacements = @(
  @('delete vertexShaderAssemblyP;', 'delete[] vertexShaderAssemblyP;'),
  @('delete fragmentShaderAssemblyP;', 'delete[] fragmentShaderAssemblyP;'),
  @('bufferP = new unsigned char[fileLength];', 'bufferP = new unsigned char[fileLength + 1];'),
  @("`t`tbufferP[bytes] = 0;", "`t`tbufferP[bytes] = 0;`r`n`t`tif (fclose(fileP) != 0) {`r`n`t`t`tdelete[] bufferP;`r`n`t`t`tbufferP = NULL;`r`n`t`t}")
)
foreach ($replacement in $replacements) {
  $needle = $replacement[0]
  $first = $source.IndexOf($needle, [StringComparison]::Ordinal)
  $last = $source.LastIndexOf($needle, [StringComparison]::Ordinal)
  if ($first -lt 0 -or $first -ne $last) {
    throw "Expected exactly one reviewed replacement for: $needle"
  }
  $source = $source.Replace($needle, $replacement[1])
}

$directory = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Force -Path $directory | Out-Null
[IO.File]::WriteAllText($OutputPath, $source, [Text.UTF8Encoding]::new($false))
