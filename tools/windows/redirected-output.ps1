param(
 [Parameter(Mandatory)][string]$CaptureScriptPath,
 [Parameter(Mandatory)][string]$Candidate,
 [Parameter(Mandatory)][string]$CandidateSha256
)
$ErrorActionPreference='Stop'
$reference='C:\Windows\System32\wbem\WMIC.exe'
$referenceHash='2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C'
$null=& ([ScriptBlock]::Create([IO.File]::ReadAllText($CaptureScriptPath))) -ManifestJson '{"cases":[]}' -Executable $reference -ExpectedSha256 $referenceHash
if((Get-FileHash -LiteralPath $Candidate -Algorithm SHA256).Hash -ne $CandidateSha256){throw 'Candidate hash mismatch'}
$root=Join-Path $env:TEMP ('WmicCompatRedirect_'+[Guid]::NewGuid().ToString('N'))
$null=[IO.Directory]::CreateDirectory($root)
$previousDirectory=[Environment]::CurrentDirectory;$previousPath=$env:PATH
$out=Join-Path $root 'stdout.bin';$err=Join-Path $root 'stderr.bin'
$definitions=@(
 @{id='redirect-table';args='os get Caption'},
 @{id='redirect-value';args='os get Caption /value'},
 @{id='redirect-csv';args='os get Caption /format:csv'},
 @{id='redirect-query-error';args='os get WmicCompatMissing'},
 @{id='redirect-help';args='/?'},
 @{id='redirect-invalid-global';args='/nonsense'},
 @{id='redirect-repeat';args='os get Caption /every:0 /repeat:2'},
 @{id='redirect-shell-query';args='';stdin="os get Caption`r`ncontext`r`nquit`r`n"},
 @{id='redirect-shell-error';args='';stdin="os get WmicCompatMissing`r`nos get Caption /value`r`nquit`r`n"},
 @{id='redirect-append';args='os get Caption';append=$true},
 @{id='redirect-method';args='process where ProcessId=4 call GetOwner'}
)
$cases=@();$errors=@();$cleanup=$false;$hashes=@{}
try {
 [Environment]::CurrentDirectory=$root
 foreach($case in $definitions){
  $script=Join-Path $root ($case.id+'.cmd')
  $redirect=if($case.append){'>>'}else{'>'}
  $text='@echo off'+"`r`n"+'wmic '+$case.args+' '+$redirect+'"'+$out+'" 2>"'+$err+'"'+"`r`nexit /b %ERRORLEVEL%`r`n"
  [IO.File]::WriteAllText($script,$text,[Text.Encoding]::ASCII)
  $hashes[$case.id]=(Get-FileHash -LiteralPath $script -Algorithm SHA256).Hash
 }
 foreach($exe in @(@{role='reference';path=$reference},@{role='candidate';path=$Candidate})){
  $env:PATH=[IO.Path]::GetDirectoryName($exe.path)+';'+$previousPath
  foreach($case in $definitions){
   foreach($file in @($out,$err)){if([IO.File]::Exists($file)){[IO.File]::Delete($file)}}
   if($case.append){[IO.File]::WriteAllBytes($out,[byte[]](255,254,83,0,13,0,10,0))}
   $script=Join-Path $root ($case.id+'.cmd')
   if((Get-FileHash -LiteralPath $script -Algorithm SHA256).Hash -cne $hashes[$case.id]){throw 'Script bytes changed'}
   $arguments='/d /s /c ""'+$script+'""'
   $result=if($case.stdin){[WmicReferenceCapture]::RunWithInput($env:ComSpec,$arguments,20000,[Text.Encoding]::ASCII.GetBytes($case.stdin))}else{[WmicReferenceCapture]::Run($env:ComSpec,$arguments,20000)}
   $result.id=$case.id
   $files=@{};foreach($file in @($out,$err)){$files[[IO.Path]::GetFileName($file)]=[Convert]::ToBase64String([IO.File]::ReadAllBytes($file))}
   $cases+=@{role=$exe.role;id=$case.id;result=$result;files=$files}
  }
 }
}catch{$errors+=$_.Exception.Message}finally{
 [Environment]::CurrentDirectory=$previousDirectory;$env:PATH=$previousPath
 if([IO.Directory]::Exists($root)){[IO.Directory]::Delete($root,$true)}
 $cleanup=![IO.Directory]::Exists($root)
}
[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes((@{cases=$cases;errors=$errors;cleanup=$cleanup;sha256=$CandidateSha256;script_hashes=$hashes}|ConvertTo-Json -Depth 15 -Compress)))
