param([Parameter(Mandatory)][string]$CaptureScriptPath,[string]$Candidate,[string]$CandidateSha256,[switch]$ReferenceOnly)
$ErrorActionPreference='Stop'
$reference='C:\Windows\System32\wbem\WMIC.exe'
$referenceHash='2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C'
$null=& ([ScriptBlock]::Create([IO.File]::ReadAllText($CaptureScriptPath))) -ManifestJson '{"cases":[]}' -Executable $reference -ExpectedSha256 $referenceHash
if(!$ReferenceOnly -and (Get-FileHash -LiteralPath $Candidate -Algorithm SHA256).Hash -ne $CandidateSha256){throw 'Candidate hash mismatch'}
$name='WmicCompatFormat_'+[Guid]::NewGuid().ToString('N')
$value='A'+[char]0x00e9+[char]0x03a9+[char]0x5b57+[char]0xd83d+[char]0xde00+'e'+[char]0x0301
$root=Join-Path $env:TEMP ('WmicCompatUnicode_'+[Guid]::NewGuid().ToString('N'))
$null=[IO.Directory]::CreateDirectory($root)
$selection='path Win32_Environment where "Name='''+$name+''' AND UserName=''<SYSTEM>''" get VariableValue'
$cases=@();$errors=@();$cleanup=$false
function OwnedObject { @(Get-CimInstance Win32_Environment -Filter "Name='$name'") }
try {
    if ((OwnedObject).Count -ne 0){throw 'Refuse to reuse an existing name'}
    $null=New-CimInstance -ClassName Win32_Environment -Property @{Name=$name;UserName='<SYSTEM>';VariableValue=$value}
    $executables=@(@{role='reference';path=$reference})
    if(!$ReferenceOnly){$executables+=@{role='candidate';path=$Candidate}}
    $out=Join-Path $root 'out.txt';$append=Join-Path $root 'append.txt';$record=Join-Path $root 'record.xml'
    $formats=@{table='';value='/value';csv='/format:csv';output='/output:"'+$out+'"';append_new='/append:"'+$append+'"';append_ansi='/append:"'+$append+'"';record='/record:"'+$record+'"'}
    foreach($exe in $executables){foreach($id in @($formats.Keys|Sort-Object)){
        $fresh=@(OwnedObject)
        if($fresh.Count -ne 1 -or $fresh[0].UserName -cne '<SYSTEM>' -or $fresh[0].VariableValue -cne $value){throw 'Owned format fixture identity/value changed'}
        foreach($file in @($out,$append,$record)){if([IO.File]::Exists($file)){[IO.File]::Delete($file)}}
        if($id -eq 'append_ansi'){[IO.File]::WriteAllBytes($append,[Text.Encoding]::ASCII.GetBytes('SEED'))}
        $arguments=if($id -in @('output','append_new','append_ansi','record')){$formats[$id]+' '+$selection}else{$selection+$(if($formats[$id]){' '+$formats[$id]}else{''})}
        $files=@{}
        $result=[WmicReferenceCapture]::Run($exe.path,$arguments,20000);$result.id='owned-format-'+$id
        foreach($file in @($out,$append,$record)){if([IO.File]::Exists($file)){$files[[IO.Path]::GetFileName($file)]=[Convert]::ToBase64String([IO.File]::ReadAllBytes($file))}}
        $cases+=@{role=$exe.role;id=$result.id;arguments=$arguments;result=$result;files=$files}
    }}
} catch {$errors+=$_.Exception.Message} finally {
    $fresh=@(OwnedObject)
    if($fresh.Count -eq 1 -and $fresh[0].UserName -ceq '<SYSTEM>' -and $fresh[0].VariableValue -ceq $value){$fresh[0]|Remove-CimInstance}
    if([IO.Directory]::Exists($root)){[IO.Directory]::Delete($root,$true)}
    $cleanup=(@(OwnedObject).Count -eq 0 -and ![IO.Directory]::Exists($root))
}
$payload=@{name=$name;value=$value;cases=$cases;errors=$errors;cleanup=$cleanup;sha256=$CandidateSha256}
[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($payload|ConvertTo-Json -Depth 14 -Compress)))
