param([Parameter(Mandatory)][string]$CaptureScriptPath,[Parameter(Mandatory)][string]$Candidate,[Parameter(Mandatory)][string]$CandidateSha256)
$ErrorActionPreference='Stop'
$reference='C:\Windows\System32\wbem\WMIC.exe'
$referenceHash='2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C'
if ((Get-FileHash -LiteralPath $Candidate -Algorithm SHA256).Hash -ne $CandidateSha256) { throw 'Candidate hash mismatch' }
# Loads the same native launcher after verifying the original's identity/signature.
$null = & ([ScriptBlock]::Create([IO.File]::ReadAllText($CaptureScriptPath))) -ManifestJson '{"cases":[]}' -Executable $reference -ExpectedSha256 $referenceHash
$owned=Join-Path 'C:\ProgramData\WmicCompat\tests' ([Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $owned | Out-Null
$batch=Join-Path $owned 'query.cmd'
$ps=Join-Path $owned 'query.ps1'
try {
    [IO.File]::WriteAllText($batch,"@echo off`r`n`"%WMIC_TOOL%`" os get Caption,Version`r`nexit /b %ERRORLEVEL%`r`n",[Text.Encoding]::ASCII)
    [IO.File]::WriteAllText($ps,'& $env:WMIC_TOOL os get Caption,Version' + "`r`n" + 'exit $LASTEXITCODE' + "`r`n",[Text.Encoding]::ASCII)
    $cases=@(
        @{id='cmd-command';exe=$env:ComSpec;args='/d /s /c ""%WMIC_TOOL%" os get Caption,Version"'},
        @{id='unchanged-batch';exe=$env:ComSpec;args=('/d /s /c ""'+$batch+'""')},
        @{id='unchanged-powershell';exe=(Join-Path $PSHOME 'powershell.exe');args=('-NoProfile -NonInteractive -ExecutionPolicy Bypass -File "'+$ps+'"')}
    )
    $results=@(foreach($case in $cases){
        $env:WMIC_TOOL=$reference
        $expected=[WmicReferenceCapture]::Run($case.exe,$case.args,20000)
        $env:WMIC_TOOL=$Candidate
        $actual=[WmicReferenceCapture]::Run($case.exe,$case.args,20000)
        $match=$expected.state -eq 'completed' -and $actual.state -eq 'completed' -and $expected.exit_status -eq $actual.exit_status -and $expected.stdout_base64 -ceq $actual.stdout_base64 -and $expected.stderr_base64 -ceq $actual.stderr_base64
        [ordered]@{id=$case.id;matched=$match;expected=$expected;actual=$actual}
    })
    $json=[ordered]@{candidate=$Candidate;sha256=$CandidateSha256;identity=[Security.Principal.WindowsIdentity]::GetCurrent().Name;cases=$results}|ConvertTo-Json -Depth 12 -Compress
    [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json))
} finally {
    Remove-Item Env:\WMIC_TOOL -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $owned -Recurse -Force
}
