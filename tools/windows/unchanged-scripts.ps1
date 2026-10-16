param([Parameter(Mandatory)][string]$CaptureScriptPath,[Parameter(Mandatory)][string]$Candidate,[Parameter(Mandatory)][string]$CandidateSha256)
$ErrorActionPreference='Stop'
$reference='C:\Windows\System32\wbem\WMIC.exe';$referenceHash='2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C'
$null=& ([ScriptBlock]::Create([IO.File]::ReadAllText($CaptureScriptPath))) -ManifestJson '{"cases":[]}' -Executable $reference -ExpectedSha256 $referenceHash
if((Get-FileHash -LiteralPath $Candidate -Algorithm SHA256).Hash -ne $CandidateSha256){throw 'Candidate hash mismatch'}
$root=Join-Path $env:TEMP ('WmicCompatScripts_'+[Guid]::NewGuid().ToString('N'));$null=[IO.Directory]::CreateDirectory($root)
$previousDirectory=[Environment]::CurrentDirectory;$previousPath=$env:PATH
$cases=@();$errors=@();$cleanup=$false;$resolutions=@();$hashes=@{}
try {
 $files=@{
  'query.cmd'="@echo off`r`nwmic os get Caption,Version`r`nexit /b %ERRORLEVEL%`r`n";
  'pipe.cmd'='@echo off'+"`r`n"+'wmic os get Caption /value | findstr /b /c:"Caption="'+"`r`nexit /b %ERRORLEVEL%`r`n";
  'error.cmd'='@echo off'+"`r`n"+'wmic os get WmicCompatMissing >NUL 2>NUL'+"`r`n"+'set "wmic_status=%ERRORLEVEL%"'+"`r`n"+'echo STATUS=%wmic_status%'+"`r`n"+'exit /b %wmic_status%'+"`r`n";
  'query.ps1'='wmic os get Caption,Version'+"`r`n"+'exit $LASTEXITCODE'+"`r`n";
  'csv.ps1'='$rows=wmic os get Caption /format:csv | ConvertFrom-Csv'+"`r`n"+'$rows | ForEach-Object {$_.Caption}'+"`r`n"+'exit $LASTEXITCODE'+"`r`n";
  'error.ps1'='$ErrorActionPreference="Continue"'+"`r`n"+'wmic os get WmicCompatMissing 2>$null'+"`r`n"+'"STATUS=$LASTEXITCODE"'+"`r`n"+'exit $LASTEXITCODE'+"`r`n";
  'hardcoded.cmd'='@echo off'+"`r`n"+'C:\Windows\System32\wbem\WMIC.exe os get Caption'+"`r`nexit /b %ERRORLEVEL%`r`n"
 }
 foreach($file in $files.Keys){$p=Join-Path $root $file;[IO.File]::WriteAllText($p,$files[$file],[Text.Encoding]::ASCII);$hashes[$file]=(Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash}
 [Environment]::CurrentDirectory=$root
 foreach($exe in @(@{role='reference';path=$reference},@{role='candidate';path=$Candidate})){
  $env:PATH=[IO.Path]::GetDirectoryName($exe.path)+';'+$previousPath
  $resolver=[WmicReferenceCapture]::Run('C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe','-NoProfile -NonInteractive -Command "(Get-Command wmic.exe -CommandType Application | Select-Object -First 1).Source"',20000)
  $resolved=[Text.Encoding]::ASCII.GetString([Convert]::FromBase64String($resolver.stdout_base64)).Trim()
  if($resolver.exit_status -ne 0 -or $resolved -ine $exe.path){throw "PATH resolution failed: $resolved"}
  $resolutions+=@{role=$exe.role;path=$resolved;sha256=(Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash}
  foreach($file in @($files.Keys|Sort-Object)){
   $p=Join-Path $root $file
   if((Get-FileHash -LiteralPath $p -Algorithm SHA256).Hash -cne $hashes[$file]){throw 'Unchanged script contents were modified'}
   if($file.EndsWith('.cmd')){$result=[WmicReferenceCapture]::Run($env:ComSpec,('/d /s /c ""'+$p+'""'),20000)}
   else{$result=[WmicReferenceCapture]::Run('C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe',('-NoProfile -NonInteractive -File "'+$p+'"'),20000)}
   $result.id=$file;$cases+=@{role=$exe.role;id=$file;result=$result}
  }
 }
} catch {$errors+=$_.Exception.Message} finally {
 [Environment]::CurrentDirectory=$previousDirectory;$env:PATH=$previousPath
 if([IO.Directory]::Exists($root)){[IO.Directory]::Delete($root,$true)}
 $cleanup=![IO.Directory]::Exists($root)
}
[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes((@{cases=$cases;errors=$errors;cleanup=$cleanup;sha256=$CandidateSha256;resolutions=$resolutions;script_hashes=$hashes;hardcoded_behavior='Literal System32 wbem path continues to use the installed Microsoft binary under either PATH'}|ConvertTo-Json -Depth 14 -Compress)))
