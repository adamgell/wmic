$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot '../signature-policy.ps1')
$certificates=[Collections.Generic.List[Security.Cryptography.X509Certificates.X509Certificate2]]::new()
function Certificate([string]$Subject,[string]$Usage) {
    $key=[Security.Cryptography.RSA]::Create(2048)
    try {
        $request=[Security.Cryptography.X509Certificates.CertificateRequest]::new($Subject,$key,[Security.Cryptography.HashAlgorithmName]::SHA256,[Security.Cryptography.RSASignaturePadding]::Pkcs1)
        $oids=[Security.Cryptography.OidCollection]::new();$null=$oids.Add([Security.Cryptography.Oid]::new($Usage))
        $request.CertificateExtensions.Add([Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]::new($oids,$true))
        $cert=$request.CreateSelfSigned([DateTimeOffset]::UtcNow.AddMinutes(-1),[DateTimeOffset]::UtcNow.AddDays(1));$certificates.Add($cert);return $cert
    } finally {$key.Dispose()}
}
function Reject([scriptblock]$Action) {
    $rejected=$false;try {& $Action} catch {$rejected=$true}
    if(!$rejected){throw 'Invalid certificate identity was accepted'}
}
try {
    Assert-CodeSigningIdentity (Certificate 'CN=Adam Gell, O=Adam Gell, C=US' '1.3.6.1.5.5.7.3.3') 'Adam Gell'
    Reject {Assert-CodeSigningIdentity (Certificate 'CN=Other, O=Adam Gell, C=US' '1.3.6.1.5.5.7.3.3') 'Adam Gell'}
    Reject {Assert-CodeSigningIdentity (Certificate 'CN=Adam Gell, O=Other, C=US' '1.3.6.1.5.5.7.3.3') 'Adam Gell'}
    Reject {Assert-CodeSigningIdentity (Certificate 'CN=Adam Gell, CN=Other, O=Adam Gell, C=US' '1.3.6.1.5.5.7.3.3') 'Adam Gell'}
    Reject {Assert-CodeSigningIdentity (Certificate 'CN=Adam Gell, O=Adam Gell, C=US' '1.3.6.1.5.5.7.3.1') 'Adam Gell'}
    Assert-TimestampIdentity (Certificate 'CN=Microsoft Public RSA Time Stamping Authority, O=Microsoft Corporation, C=US' '1.3.6.1.5.5.7.3.8')
    Reject {Assert-TimestampIdentity (Certificate 'CN=Other, O=Other, C=US' '1.3.6.1.5.5.7.3.8')}
    Reject {Assert-TimestampIdentity (Certificate 'CN=Microsoft Public RSA Time Stamping Authority, O=Microsoft Corporation, C=US' '1.3.6.1.5.5.7.3.3')}
    '8 certificate identity cases passed.'
} finally {foreach($cert in $certificates){$cert.Dispose()}}
