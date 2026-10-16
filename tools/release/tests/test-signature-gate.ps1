#Requires -Version 7.3
param(
    [Parameter(Mandatory)][string]$Binary,
    [Parameter(Mandatory)][string]$Target,
    [Parameter(Mandatory)][ValidateSet('Unsigned','Signed')][string]$Mode,
    [string]$ExpectedPublisher='Adam Gell'
)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
$temporary=Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid().ToString())
$null=New-Item -ItemType Directory $temporary
$verifier=Join-Path $PSScriptRoot '../verify-signature.ps1'
function Assert-Rejected([string]$Path,[string]$Publisher,[string]$Name) {
    $receipt=Join-Path $temporary "$Name.json"
    $rejected=$false
    try { & $verifier -Binary $Path -Target $Target -SourceCommit $env:GITHUB_SHA -ExpectedPublisher $Publisher -ReceiptPath $receipt }
    catch { $rejected=$true; Write-Host "Correctly rejected $Name : $_" }
    if(!$rejected -or (Test-Path -LiteralPath $receipt)){throw "Signature gate accepted $Name"}
}
try {
    if($Mode -eq 'Unsigned') {
        if((Get-AuthenticodeSignature -LiteralPath $Binary).Status -ne 'NotSigned'){throw 'Unsigned test requires an unsigned executable'}
        Assert-Rejected $Binary $ExpectedPublisher 'unsigned'
    } else {
        if((Get-AuthenticodeSignature -LiteralPath $Binary).Status -ne 'Valid'){throw 'Signed test requires a trusted executable'}
        Assert-Rejected $Binary 'Wrong Publisher Fixture' 'wrong-publisher'
        $bytes=[IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Binary))
        $pe=[BitConverter]::ToInt32($bytes,60)
        $optionalSize=[BitConverter]::ToUInt16($bytes,$pe+20)
        $section=$pe+24+$optionalSize
        $rawOffset=[BitConverter]::ToInt32($bytes,$section+20)
        if($rawOffset -le 0 -or $rawOffset -ge $bytes.Length){throw 'Invalid PE section fixture'}
        $bytes[$rawOffset]=$bytes[$rawOffset] -bxor 1
        $tampered=Join-Path $temporary 'tampered.exe'
        [IO.File]::WriteAllBytes($tampered,$bytes)
        if((Get-AuthenticodeSignature -LiteralPath $tampered).Status -eq 'Valid'){throw 'Tampered test did not invalidate the signature'}
        Assert-Rejected $tampered $ExpectedPublisher 'tampered'
    }
} finally { Remove-Item -LiteralPath $temporary -Recurse -Force }
