# Identity checks supplement Windows trust and SignTool; they do not establish trust themselves.
function Assert-CertificateFields {
    param($Certificate,[hashtable]$Expected)
    $found=@{}
    foreach($rdn in $Certificate.SubjectName.EnumerateRelativeDistinguishedNames()) {
        if($rdn.HasMultipleElements){throw 'Ambiguous certificate name'}
        $oid=$rdn.GetSingleElementType().Value
        if($Expected.ContainsKey($oid)) {
            if($found.ContainsKey($oid)){throw "Duplicate certificate identity field $oid"}
            $found[$oid]=$rdn.GetSingleElementValue()
        }
    }
    foreach($oid in $Expected.Keys) {
        if(!$found.ContainsKey($oid) -or $found[$oid] -cne $Expected[$oid]){throw "Unexpected certificate identity field $oid"}
    }
}
function Assert-CertificateUsage {
    param($Certificate,[string]$Usage)
    $extensions=@($Certificate.Extensions | Where-Object {$_.Oid.Value -eq '2.5.29.37'})
    if($extensions.Count -ne 1){throw 'Missing or ambiguous extended key usage'}
    $usageExtension=[Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension]::new($extensions[0],$extensions[0].Critical)
    if($Usage -notin @($usageExtension.EnhancedKeyUsages | ForEach-Object {$_.Value})){throw 'Unexpected certificate usage'}
}
function Assert-CodeSigningIdentity {
    param($Certificate,[string]$ExpectedPublisher)
    if([string]::IsNullOrWhiteSpace($ExpectedPublisher)){throw 'Expected publisher is required'}
    Assert-CertificateFields $Certificate @{'2.5.4.3'=$ExpectedPublisher;'2.5.4.10'=$ExpectedPublisher;'2.5.4.6'='US'}
    Assert-CertificateUsage $Certificate '1.3.6.1.5.5.7.3.3'
}
function Assert-TimestampIdentity {
    param($Certificate)
    Assert-CertificateFields $Certificate @{'2.5.4.3'='Microsoft Public RSA Time Stamping Authority';'2.5.4.10'='Microsoft Corporation';'2.5.4.6'='US'}
    Assert-CertificateUsage $Certificate '1.3.6.1.5.5.7.3.8'
}
