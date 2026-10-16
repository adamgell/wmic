#Requires -Version 7.3
param(
    [Parameter(Mandatory)][string]$Binary,
    [Parameter(Mandatory)][ValidateSet('aarch64-pc-windows-msvc','x86_64-pc-windows-msvc','i686-pc-windows-msvc')][string]$Target,
    [Parameter(Mandatory)][string]$SourceCommit,
    [Parameter(Mandatory)][string]$ExpectedPublisher,
    [Parameter(Mandatory)][string]$ReceiptPath
)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'signature-policy.ps1')
if($SourceCommit -cnotmatch '^[0-9a-f]{40}$' -or $env:GITHUB_SHA -cne $SourceCommit -or $env:GITHUB_RUN_ID -notmatch '^[1-9][0-9]*$' -or $env:GITHUB_RUN_ATTEMPT -notmatch '^[1-9][0-9]*$'){throw 'Invalid signing source/run context'}
$file=Get-Item -LiteralPath $Binary
if($file -isnot [IO.FileInfo] -or ($file.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Executable must be a regular file'}
$before=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
$signature=Get-AuthenticodeSignature -LiteralPath $file.FullName
if($signature.Status -ne 'Valid' -or $signature.SignatureType -ne 'Authenticode'){throw "Authenticode verification failed: $($signature.Status)"}
if($null -eq $signature.SignerCertificate -or $null -eq $signature.TimeStamperCertificate){throw 'Publisher or timestamp certificate is missing'}
Assert-CodeSigningIdentity $signature.SignerCertificate $ExpectedPublisher
Assert-TimestampIdentity $signature.TimeStamperCertificate
$sdkRoot="${env:ProgramFiles(x86)}\Windows Kits\10\bin"
$tools=@(Get-ChildItem -LiteralPath $sdkRoot -Directory | ForEach-Object {
    $version=$null
    if([version]::TryParse($_.Name,[ref]$version)) {
        $path=Join-Path $_.FullName 'x64/signtool.exe'
        if(Test-Path -LiteralPath $path -PathType Leaf){[pscustomobject]@{path=$path;version=$version}}
    }
} | Sort-Object version -Descending)
if($tools.Count -eq 0){throw 'Windows SDK x64 SignTool is missing'}
$verification=& $tools[0].path verify /pa /all /v /tw $file.FullName 2>&1
$status=$LASTEXITCODE
if($status -ne 0){$verification | Write-Output;throw "SignTool verification failed: $status"}
$after=(Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash.ToLowerInvariant()
if($before -cne $after){throw 'Executable bytes changed during signature verification'}
$receipt=[ordered]@{
    schema_version=1;source_commit=$SourceCommit;target=$Target;run_id=$env:GITHUB_RUN_ID;run_attempt=$env:GITHUB_RUN_ATTEMPT
    file=@{sha256=$after;bytes=$file.Length}
    verification=@{
        authenticode_status=[string]$signature.Status;signature_type=[string]$signature.SignatureType;signtool_exit_code=$status
        publisher=$ExpectedPublisher;timestamp_present=$true;signer_thumbprint=$signature.SignerCertificate.Thumbprint;timestamp_thumbprint=$signature.TimeStamperCertificate.Thumbprint
        signer_subject=$signature.SignerCertificate.Subject;signer_issuer=$signature.SignerCertificate.Issuer
        timestamp_subject=$signature.TimeStamperCertificate.Subject;timestamp_issuer=$signature.TimeStamperCertificate.Issuer
    }
}
$receiptFull=[IO.Path]::GetFullPath($ReceiptPath)
$null=[IO.Directory]::CreateDirectory([IO.Path]::GetDirectoryName($receiptFull))
$stream=[IO.File]::Open($receiptFull,[IO.FileMode]::CreateNew)
$writer=[IO.StreamWriter]::new($stream)
try{$writer.WriteLine(($receipt|ConvertTo-Json -Depth 8))}finally{$writer.Dispose()}
"Verified ${Target}: $after"
