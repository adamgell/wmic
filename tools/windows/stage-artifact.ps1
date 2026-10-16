param([Parameter(Mandatory)][string]$SourcePath,[Parameter(Mandatory)][ValidatePattern('^[0-9a-fA-F]{64}$')][string]$ExpectedSha256,[string]$FileName='wmic.exe')
$ErrorActionPreference = 'Stop'
if ($FileName -notmatch '^[A-Za-z0-9_.-]+\.exe$') { throw 'Invalid artifact file name' }
$hash = (Get-FileHash -LiteralPath $SourcePath -Algorithm SHA256).Hash
if ($hash -ne $ExpectedSha256) { throw 'Source artifact hash mismatch' }
$directory = Join-Path 'C:\ProgramData\WmicCompat\artifacts' $hash
$destination = Join-Path $directory $FileName
New-Item -ItemType Directory -Force -Path $directory | Out-Null
if (Test-Path -LiteralPath $destination) {
    if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $hash) { throw 'Existing artifact hash mismatch' }
} else { Copy-Item -LiteralPath $SourcePath -Destination $destination }
if ((Get-FileHash -LiteralPath $destination -Algorithm SHA256).Hash -ne $hash) { throw 'Staged artifact hash mismatch' }
$destination
