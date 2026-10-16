param([Parameter(Mandatory)][string]$CaptureScriptPath,[string]$Candidate,[string]$CandidateSha256,[switch]$ReferenceOnly,[switch]$Whitespace)
$ErrorActionPreference='Stop'
$reference='C:\Windows\System32\wbem\WMIC.exe'
$referenceHash='2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C'
$null=& ([ScriptBlock]::Create([IO.File]::ReadAllText($CaptureScriptPath))) -ManifestJson '{"cases":[]}' -Executable $reference -ExpectedSha256 $referenceHash
if(!$ReferenceOnly -and (Get-FileHash -LiteralPath $Candidate -Algorithm SHA256).Hash -ne $CandidateSha256){throw 'Candidate hash mismatch'}
$name='WmicCompatFormat_'+[Guid]::NewGuid().ToString('N')
$value='a,b"c & <tag>'
if($Whitespace){$value='  '+$value+'  '}
$selection='path Win32_Environment where "Name='''+$name+''' AND UserName=''<SYSTEM>''" get VariableValue'
$cases=@();$errors=@();$cleanup=$false
function OwnedObject { @(Get-CimInstance Win32_Environment -Filter "Name='$name'") }
try {
    if ((OwnedObject).Count -ne 0){throw 'Refuse to reuse an existing name'}
    $null=New-CimInstance -ClassName Win32_Environment -Property @{Name=$name;UserName='<SYSTEM>';VariableValue=$value}
    $executables=@(@{role='reference';path=$reference})
    if(!$ReferenceOnly){$executables+=@{role='candidate';path=$Candidate}}
    $formats=@{table='';value='/value';csv='/format:csv';mof='/format:mof';rawxml='/format:rawxml';xml='/format:xml';htable='/format:htable';hform='/format:hform';basicxml_before='/translate:BasicXml /format:csv';basicxml_after='/format:csv /translate:BasicXml';nocomma_before='/translate:NoComma /format:csv';nocomma_after='/format:csv /translate:NoComma'}
    foreach($exe in $executables){foreach($id in @($formats.Keys|Sort-Object)){
        $fresh=@(OwnedObject)
        if($fresh.Count -ne 1 -or $fresh[0].UserName -cne '<SYSTEM>' -or $fresh[0].VariableValue -cne $value){throw 'Owned format fixture identity/value changed'}
        $arguments=$selection+$(if($formats[$id]){' '+$formats[$id]}else{''})
        $result=[WmicReferenceCapture]::Run($exe.path,$arguments,20000);$result.id='owned-format-'+$id
        $cases+=@{role=$exe.role;id=$result.id;arguments=$arguments;result=$result}
    }}
} catch {$errors+=$_.Exception.Message} finally {
    $fresh=@(OwnedObject)
    if($fresh.Count -eq 1 -and $fresh[0].UserName -ceq '<SYSTEM>' -and $fresh[0].VariableValue -ceq $value){$fresh[0]|Remove-CimInstance}
    $cleanup=@(OwnedObject).Count -eq 0
}
$payload=@{name=$name;value=$value;cases=$cases;errors=$errors;cleanup=$cleanup;sha256=$CandidateSha256}
[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($payload|ConvertTo-Json -Depth 14 -Compress)))
