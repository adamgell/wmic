param([Parameter(Mandatory)][string]$SourcePath,[Parameter(Mandatory)][string]$Candidate,[Parameter(Mandatory)][string]$CandidateSha256,[switch]$TypeaheadOnly)
$ErrorActionPreference='Stop'
$reference='C:\Windows\System32\wbem\WMIC.exe'
if((Get-FileHash -LiteralPath $reference -Algorithm SHA256).Hash -ne '2EF794C4940AFEA56C6D6D3B9BC9E66965BC7333E79C5DCB2DEF3C0E36F0834C'){throw 'Reference hash changed'}
if((Get-FileHash -LiteralPath $Candidate -Algorithm SHA256).Hash -ne $CandidateSha256){throw 'Candidate hash changed'}
$root=Join-Path $env:TEMP ('WmicCompatConsole_'+[Guid]::NewGuid().ToString('N'));$null=[IO.Directory]::CreateDirectory($root)
$exe=Join-Path $root 'console-probe.exe';$report=Join-Path $root 'result.txt';$cases=@();$errors=@();$cleanup=$false
# CreateProcessW explicitly requests a new hidden console. No user console is touched.
$launcher=@'
using System;using System.Text;using System.Runtime.InteropServices;
public static class WmicPrivateConsole {
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)]struct S{public int cb;public string r,d,t;public int x,y,w,h,xc,yc,f,flags;public short show,r2;public IntPtr p,i,o,e;}
 [StructLayout(LayoutKind.Sequential)]struct P{public IntPtr h,t;public uint id,tid;}
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool CreateProcessW(string e,StringBuilder c,IntPtr a,IntPtr b,bool inherit,uint f,IntPtr env,string cwd,ref S s,out P p);
 [DllImport("kernel32")]static extern uint WaitForSingleObject(IntPtr h,uint t);
 [DllImport("kernel32")]static extern bool TerminateProcess(IntPtr h,uint c);
 [DllImport("kernel32")]static extern bool CloseHandle(IntPtr h);
 public static void Run(string exe,string command,string cwd){S s=new S();s.cb=Marshal.SizeOf(typeof(S));s.flags=1;s.show=0;P p;if(!CreateProcessW(exe,new StringBuilder(command),IntPtr.Zero,IntPtr.Zero,false,0x10,IntPtr.Zero,cwd,ref s,out p))throw new Exception("Private console creation failed: "+Marshal.GetLastWin32Error());try{if(WaitForSingleObject(p.h,30000)!=0){TerminateProcess(p.h,0xffffffff);throw new Exception("Private console probe timed out");}}finally{CloseHandle(p.t);CloseHandle(p.h);}}
}
'@
try {
 Add-Type -TypeDefinition ([IO.File]::ReadAllText($SourcePath)) -OutputAssembly $exe -OutputType ConsoleApplication
 Add-Type -TypeDefinition $launcher
 $definitions=@(@{id='console-context';arguments='';input="context`rquit`r"},@{id='console-query';arguments='';input="os get Caption`rquit`r"},@{id='console-empty';arguments='';input="`rquit`r"},@{id='console-cancel-repeat';arguments='os get Caption /every:30';input='';mode='cancel'},@{id='console-cancel-second';arguments='os get Caption /every:1';input='';mode='cancel-second'},@{id='console-break-key';arguments='os get Caption /every:30';input='';mode='break-key'},@{id='console-eof';arguments='';input=([string][char]26)+"`r"},@{id='console-repeat2';arguments='os get Caption /every:0 /repeat:2';input='';mode='noinput'},@{id='console-repeat2-wait';arguments='os get Caption /every:1 /repeat:2';input='';mode='noinput'})
 if($TypeaheadOnly){$definitions=@(@{id='console-typeahead';arguments='';input="os get Caption`rquit`r";mode='typeahead'})}
 foreach($role in @(@{name='reference';path=$reference},@{name='candidate';path=$Candidate})) {foreach($case in $definitions){
  if([IO.File]::Exists($report)){[IO.File]::Delete($report)}
  $input64=[Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($case.input));if(!$input64){$input64='AA=='}
  $mode=if($case.mode){$case.mode}else{'input'}
  $command='"'+$exe+'" "'+$role.path+'" "'+$case.arguments+'" "'+$input64+'" "'+$report+'" '+$mode
  [WmicPrivateConsole]::Run($exe,$command,$root)
  $lines=[IO.File]::ReadAllLines($report)
  if($lines[0] -eq 'error'){throw ($lines -join "`n")}
  $cases+=@{id=$case.id;role=$role.name;state=$lines[0];exit_status=$(if($lines[0] -eq 'completed'){[uint32]$lines[1]}else{$null});termination_status=$(if($lines[0] -ne 'completed'){[uint32]$lines[1]}else{$null});screen_utf16_base64=$lines[2];width=[int]$lines[3];cursor_x=[int]$lines[4];cursor_y=[int]$lines[5]}
 }}
} catch {$errors+=$_.Exception.Message} finally {if([IO.Directory]::Exists($root)){[IO.Directory]::Delete($root,$true)};$cleanup=![IO.Directory]::Exists($root)}
[Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes((@{cases=$cases;errors=$errors;cleanup=$cleanup;sha256=$CandidateSha256;type='private Windows console screen and real console input events'}|ConvertTo-Json -Depth 12 -Compress)))
