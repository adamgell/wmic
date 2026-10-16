param([Parameter(Mandatory)][string]$CaptureScriptPath,[Parameter(Mandatory)][string]$Candidate,[Parameter(Mandatory)][string]$CandidateSha256,[switch]$ReferenceOnly,[switch]$NamedMethodInput)
$ErrorActionPreference='Stop'
$reference='C:\Windows\System32\wbem\WMIC.exe'
$referenceHash='2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C'
if (!$ReferenceOnly -and (Get-FileHash -LiteralPath $Candidate -Algorithm SHA256).Hash -ne $CandidateSha256) {throw 'Candidate hash mismatch'}
$null=& ([ScriptBlock]::Create([IO.File]::ReadAllText($CaptureScriptPath))) -ManifestJson '{"cases":[]}' -Executable $reference -ExpectedSha256 $referenceHash
$token='WmicCompat_'+[Guid]::NewGuid().ToString('N')
$name=$token+'_Environment'
$ps=Join-Path $PSHOME 'powershell.exe'
$script='$ownedMarker='''+$token+'''; Start-Sleep -Seconds 120'
$encoded=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($script))
$ownedCommand=$ps+' -NoProfile -NoLogo -NonInteractive -EncodedCommand '+$encoded
$selection='path Win32_Environment where "Name='''+$name+''' AND UserName=''<SYSTEM>''"'
$results=@();$errors=@();$cleanup=@();$probes=@()
function EnvironmentObject { @(Get-CimInstance -ClassName Win32_Environment -Filter "Name='$name'") }
function AssertEnvironment([string]$value) {
    $objects=@(EnvironmentObject)
    if ($objects.Count -ne 1 -or $objects[0].Name -cne $name -or $objects[0].UserName -cne '<SYSTEM>' -or $objects[0].VariableValue -cne $value) {throw 'Owned environment identity/value changed'}
    $objects[0]
}
function RunCase([string]$role,[string]$exe,[string]$id,[string]$arguments,[string]$inputText='') {
    $r=[WmicReferenceCapture]::RunWithInput($exe,$arguments,20000,[Text.Encoding]::ASCII.GetBytes($inputText))
    $r.id=$id
    if ($r.state -ne 'completed') {throw ('Owned test timed out: '+$id)}
    [ordered]@{role=$role;id=$id;arguments=$arguments;result=$r}
}
$executables=@(@{role='reference';path=$reference})
if (!$ReferenceOnly) {$executables+=@{role='candidate';path=$Candidate}}
foreach($exe in $executables) {
    $pidOwned=$null;$creation=$null
    try {
        if ((EnvironmentObject).Count -ne 0) {throw 'Refuse to reuse an existing environment name'}
        $create='path Win32_Environment create Name="'+$name+'",UserName="<SYSTEM>",VariableValue="initial value"'
        $results+=RunCase $exe.role $exe.path 'environment-create' $create
        $null=AssertEnvironment 'initial value'
        $results+=RunCase $exe.role $exe.path 'environment-get' ($selection+' get Name,UserName,VariableValue /value')
        $null=AssertEnvironment 'initial value'
        $results+=RunCase $exe.role $exe.path 'environment-set-declined' ('/interactive:on '+$selection+' set VariableValue="must remain unchanged"') "N`r`n"
        $null=AssertEnvironment 'initial value'
        $results+=RunCase $exe.role $exe.path 'environment-set' ($selection+' set VariableValue="changed value"')
        $null=AssertEnvironment 'changed value'
        $results+=RunCase $exe.role $exe.path 'environment-get-changed' ($selection+' get Name,UserName,VariableValue /value')
        $null=AssertEnvironment 'changed value'
        $results+=RunCase $exe.role $exe.path 'environment-delete-help' ($selection+' delete /interactive') "?`r`nN`r`n"
        $null=AssertEnvironment 'changed value'
        $results+=RunCase $exe.role $exe.path 'environment-delete-declined' ($selection+' delete /interactive') "N`r`n"
        $null=AssertEnvironment 'changed value'
        $results+=RunCase $exe.role $exe.path 'environment-delete' ($selection+' delete /nointeractive')
        if ((EnvironmentObject).Count -ne 0) {throw 'Environment delete did not remove the owned object'}
        if($NamedMethodInput){
            $probes+=RunCase $exe.role $exe.path 'process-create-null-directory' ('process call Create CurrentDirectory=NULL CommandLine="'+$ownedCommand+'"')
            if (@(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object {$_.CommandLine -ceq $ownedCommand}).Count -ne 0) {throw 'NULL directory probe unexpectedly created a process'}
        }
        $createArguments=if($NamedMethodInput){'process call Create CurrentDirectory="'+$env:SystemRoot+'" CommandLine="'+$ownedCommand+'"'}else{'process call Create "'+$ownedCommand+'"'}
        $results+=RunCase $exe.role $exe.path 'process-create' $createArguments
        $objects=@(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object {$_.CommandLine -ceq $ownedCommand})
        if ($objects.Count -ne 1 -or $objects[0].ExecutablePath -ine $ps) {throw 'Owned process identity missing or ambiguous'}
        $pidOwned=[uint32]$objects[0].ProcessId;$creation=$objects[0].CreationDate
        $results+=RunCase $exe.role $exe.path 'process-owner' ('process where ProcessId='+$pidOwned+' call GetOwner')
        $current=Get-CimInstance Win32_Process -Filter "ProcessId=$pidOwned"
        if ($null -eq $current -or $current.CommandLine -cne $ownedCommand -or $current.CreationDate -ne $creation) {throw 'Process identity changed before termination'}
        $results+=RunCase $exe.role $exe.path 'process-terminate' ('process where ProcessId='+$pidOwned+' call Terminate')
        if (Get-CimInstance Win32_Process -Filter "ProcessId=$pidOwned") {throw 'Owned process still present after Terminate'}
    } catch {$errors+=($exe.role+': '+$_.Exception.Message)} finally {
        $envObjects=@(EnvironmentObject)
        if ($envObjects.Count -eq 1 -and $envObjects[0].Name -ceq $name -and $envObjects[0].UserName -ceq '<SYSTEM>') {$envObjects[0] | Remove-CimInstance}
        $processes=@(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object {$_.CommandLine -ceq $ownedCommand})
        foreach($owned in $processes) {
            $fresh=Get-CimInstance Win32_Process -Filter ('ProcessId='+$owned.ProcessId)
            if ($null -ne $fresh -and $fresh.CommandLine -ceq $ownedCommand -and $fresh.CreationDate -eq $owned.CreationDate) {Stop-Process -Id $fresh.ProcessId -Force}
        }
        $remaining=@(EnvironmentObject).Count
        $remainingProcesses=@(Get-CimInstance Win32_Process -Filter "Name='powershell.exe'" | Where-Object {$_.CommandLine -ceq $ownedCommand}).Count
        $cleanup+=@{role=$exe.role;process_id=$pidOwned;creation_date=$creation;environment_remaining=$remaining;process_remaining=$remainingProcesses}
    }
}
$payload=[ordered]@{token=$token;name=$name;owned_command=$ownedCommand;sha256=$CandidateSha256;identity=[Security.Principal.WindowsIdentity]::GetCurrent().Name;cases=$results;probes=$probes;cleanup=$cleanup;errors=$errors}
[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($payload|ConvertTo-Json -Depth 14 -Compress)))
