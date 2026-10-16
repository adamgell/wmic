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
 @{id='output-new';args='/output:"'+$out+'" os get Caption'},
 @{id='output-existing';args='/output:"'+$out+'" os get Caption';seed='out.txt'},
 @{id='append-new';args='/append:"'+$append+'" os get Caption'},
 @{id='append-existing';args='/append:"'+$append+'" os get Caption';seed='append.txt'},
 @{id='both-output-append';args='/output:"'+$out+'" /append:"'+$append+'" os get Caption';seed='append.txt'},
 @{id='both-append-output';args='/append:"'+$append+'" /output:"'+$out+'" os get Caption';seed='append.txt'},
 @{id='output-stdout';args='/output:stdout os get Caption'},
 @{id='append-stdout';args='/append:stdout os get Caption'},
 @{id='output-reset';args='/output:"'+$out+'" /output: os get Caption'},
 @{id='append-reset';args='/append:"'+$append+'" /append: os get Caption'},
 @{id='append-invalid-property';args='/append:"'+$append+'" os get WmicCompatMissing'},
 @{id='output-invalid-property';args='/output:"'+$out+'" os get WmicCompatMissing'},
 @{id='output-missing-dir';args='/output:"'+$root+'\missing\out.txt" os get Caption'},
 @{id='output-is-directory';args='/output:"'+$root+'" os get Caption'},
 @{id='record-new';args='/record:"'+$record+'" os get Caption'},
 @{id='record-existing';args='/record:"'+$record+'" os get Caption';seed='record.xml'},
 @{id='record-output';args='/record:"'+$record+'" /output:"'+$out+'" os get Caption'},
 @{id='record-error';args='/record:"'+$record+'" os get WmicCompatMissing'},
 @{id='record-reset';args='/record:"'+$record+'" /record: os get Caption'},
 @{id='output-csv';args='/output:"'+$out+'" os get Caption /format:csv'},
 @{id='append-utf16';args='/append:"'+$append+'" os get Caption';utf16='append.txt'},
 @{id='append-utf8';args='/append:"'+$append+'" os get Caption';utf8='append.txt'}
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
  $result=[WmicReferenceCapture]::Run($exe.path,$case.args,20000);$result.id=$case.id
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
