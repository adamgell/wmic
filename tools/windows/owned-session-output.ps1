param([Parameter(Mandatory)][string]$CaptureScriptPath,[string]$Candidate,[string]$CandidateSha256,[switch]$ReferenceOnly)
$ErrorActionPreference='Stop'
$reference='C:\Windows\System32\wbem\WMIC.exe'
$referenceHash='2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C'
$null=& ([ScriptBlock]::Create([IO.File]::ReadAllText($CaptureScriptPath))) -ManifestJson '{"cases":[]}' -Executable $reference -ExpectedSha256 $referenceHash
if(!$ReferenceOnly -and (Get-FileHash -LiteralPath $Candidate -Algorithm SHA256).Hash -ne $CandidateSha256){throw 'Candidate hash mismatch'}
$root=Join-Path $env:TEMP ('WmicCompatOutput_'+[Guid]::NewGuid().ToString('N'))
$null=[IO.Directory]::CreateDirectory($root)
$previousDirectory=[Environment]::CurrentDirectory;[Environment]::CurrentDirectory=$root
$out=Join-Path $root 'out.txt';$append=Join-Path $root 'append.txt';$record=Join-Path $root 'record.xml'
$sentinel=Join-Path $root 'unrelated.bin';[IO.File]::WriteAllBytes($sentinel,[byte[]](0,255,13,10,42))
$definitions=@(
 @{id='shell-output';args='';stdin='/output:"'+$out+'"'+"`r`nos get Caption`r`n/output:STDOUT`r`nos get Version`r`nquit`r`n"},
 @{id='shell-output-quit';args='';stdin='/output:"'+$out+'"'+"`r`nos get Caption`r`nquit`r`n"},
 @{id='shell-output-bad';args='';stdin='/output:"'+$out+'"'+"`r`nos get Caption`r`n/nonsense`r`nquit`r`n"},
 @{id='shell-record-bad';args='';stdin='/record:"'+$record+'"'+"`r`nos get Caption`r`n/nonsense`r`nquit`r`n"},
 @{id='shell-append';args='';stdin='/append:"'+$append+'"'+"`r`nos get Caption`r`nos get Version`r`nquit`r`n"},
 @{id='shell-record';args='';stdin='/record:"'+$record+'"'+"`r`nos get Caption`r`nos get Version`r`nquit`r`n"},
 @{id='shell-inline-output';args='';stdin='/output:"'+$out+'" os get Caption'+"`r`nos get Version`r`nquit`r`n"},
 @{id='repeat-output';args='/output:"'+$out+'" os get Caption /every:0 /repeat:2'},
 @{id='repeat-record';args='/record:"'+$record+'" os get Caption /every:0 /repeat:2'}
)
$cases=@();$errors=@();$cleanup=$false
try {
 $executables=@(@{role='reference';path=$reference})
 if(!$ReferenceOnly){$executables+=@{role='candidate';path=$Candidate}}
 foreach($exe in $executables){foreach($case in $definitions){
  foreach($file in @(Get-ChildItem -LiteralPath $root -File|Where-Object Name -ne 'unrelated.bin')){[IO.File]::Delete($file.FullName)}
  if($case.seed){[IO.File]::WriteAllBytes((Join-Path $root $case.seed),[byte[]](83,69,69,68,13,10))}
  if($case.utf16){[IO.File]::WriteAllBytes((Join-Path $root $case.utf16),[byte[]](255,254,83,0,13,0,10,0))}
  if($case.utf8){[IO.File]::WriteAllBytes((Join-Path $root $case.utf8),[byte[]](239,187,191,83,13,10))}
  $result=if($case.stdin){[WmicReferenceCapture]::RunWithInput($exe.path,$case.args,20000,[Text.Encoding]::ASCII.GetBytes($case.stdin))}else{[WmicReferenceCapture]::Run($exe.path,$case.args,20000)};$result.id=$case.id
  $files=@{};foreach($file in @(Get-ChildItem -LiteralPath $root -File)){$files[$file.Name]=[Convert]::ToBase64String([IO.File]::ReadAllBytes($file.FullName))}
  if($files['unrelated.bin'] -cne 'AP8NCio='){throw 'Unrelated sentinel was changed'}
  $cases+=@{role=$exe.role;id=$case.id;arguments=$case.args;result=$result;files=$files}
 }}
} catch {$errors+=$_.Exception.Message} finally {
 [Environment]::CurrentDirectory=$previousDirectory
 if([IO.Directory]::Exists($root)){[IO.Directory]::Delete($root,$true)}
 $cleanup=![IO.Directory]::Exists($root)
}
$payload=@{root=$root;cases=$cases;errors=$errors;cleanup=$cleanup;sha256=$CandidateSha256}
[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes(($payload|ConvertTo-Json -Depth 15 -Compress)))
