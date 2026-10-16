param(
    [Parameter(Mandatory=$true)][string]$ManifestJson,
    [Parameter(Mandatory=$true)][string]$Executable,
    [Parameter(Mandatory=$true)][string]$ExpectedSha256,
    [ValidateSet('reference','candidate')][string]$Role = 'reference'
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'

try {
    $file = Get-Item -LiteralPath $Executable
    $hash = (Get-FileHash -LiteralPath $Executable -Algorithm SHA256).Hash
    if ($hash -ne $ExpectedSha256) { throw 'Executable SHA-256 does not match the requested identity.' }
    $signature = Get-AuthenticodeSignature -LiteralPath $Executable
    if ($Role -eq 'reference' -and ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '^CN=Microsoft Windows,')) {
        throw 'Reference mode requires a valid Microsoft Windows signature; use candidate mode for a replacement.'
    }

    Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading.Tasks;
using Microsoft.Win32.SafeHandles;

public sealed class WmicCaptureResult {
    public string id;
    public string arguments_utf16_base64;
    public string command_line_utf16_base64;
    public string state;
    public uint? exit_status;
    public long duration_ms;
    public uint process_id;
    public string stdout_base64;
    public string stderr_base64;
    public object[] normalizations;
}

public static class WmicReferenceCapture {
    [StructLayout(LayoutKind.Sequential)] struct SecurityAttributes {
        public int Length; public IntPtr Descriptor; public int Inherit;
    }
    [StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)] struct StartupInfo {
        public int cb;
        public string reserved, desktop, title;
        public int x, y, xSize, ySize, xChars, yChars, fill, flags;
        public short show, reservedLength;
        public IntPtr reservedBytes, stdin, stdout, stderr;
    }
    [StructLayout(LayoutKind.Sequential)] struct ProcessInfo {
        public IntPtr process, thread; public uint processId, threadId;
    }
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CreatePipe(out IntPtr read, out IntPtr write, ref SecurityAttributes attributes, int size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetHandleInformation(IntPtr handle, uint mask, uint flags);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CreateProcessW(string application, StringBuilder commandLine, IntPtr processAttributes, IntPtr threadAttributes, bool inherit, uint flags, IntPtr environment, string directory, ref StartupInfo startup, out ProcessInfo process);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr handle, uint milliseconds);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr handle, out uint status);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(IntPtr handle, uint status);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")] public static extern uint GetACP();
    [DllImport("kernel32.dll")] public static extern uint GetOEMCP();
    [DllImport("kernel32.dll")] public static extern uint GetConsoleOutputCP();

    static void Check(bool result) { if (!result) throw new Win32Exception(Marshal.GetLastWin32Error()); }
    static void Close(ref IntPtr handle) { if (handle != IntPtr.Zero) { CloseHandle(handle); handle=IntPtr.Zero; } }
    public static string WideBase64(string value) {
        char[] units=value.ToCharArray(); byte[] bytes=new byte[units.Length*2];
        Buffer.BlockCopy(units,0,bytes,0,bytes.Length); return Convert.ToBase64String(bytes);
    }
    public static string DecodeWide(string value) {
        byte[] bytes=Convert.FromBase64String(value);
        if (bytes.Length%2!=0) throw new ArgumentException("UTF-16 bytes must have even length");
        char[] units=new char[bytes.Length/2]; Buffer.BlockCopy(bytes,0,units,0,bytes.Length); return new string(units);
    }
    public static WmicCaptureResult Run(string executable, string arguments, int timeoutMs) {
        return RunWithInput(executable,arguments,timeoutMs,new byte[0]);
    }
    public static WmicCaptureResult RunWithInput(string executable, string arguments, int timeoutMs, byte[] inputBytes) {
        if (timeoutMs<=0 || executable.IndexOf('"')>=0 || executable.IndexOf('\0')>=0 || arguments.IndexOf('\0')>=0) throw new ArgumentException("Invalid launch arguments");
        string raw="\""+executable+"\""+(arguments.Length==0 ? "" : " "+arguments);
        if (raw.Length>=32767) throw new ArgumentException("Command line exceeds Windows limit");
        IntPtr outR=IntPtr.Zero,outW=IntPtr.Zero,errR=IntPtr.Zero,errW=IntPtr.Zero,inR=IntPtr.Zero,inW=IntPtr.Zero;
        ProcessInfo process=new ProcessInfo();
        var security=new SecurityAttributes(); security.Length=Marshal.SizeOf(security); security.Inherit=1;
        var timer=Stopwatch.StartNew();
        try {
            Check(CreatePipe(out outR,out outW,ref security,0));
            Check(CreatePipe(out errR,out errW,ref security,0));
            Check(CreatePipe(out inR,out inW,ref security,0));
            Check(SetHandleInformation(outR,1,0)); Check(SetHandleInformation(errR,1,0)); Check(SetHandleInformation(inW,1,0));
            var startup=new StartupInfo(); startup.cb=Marshal.SizeOf(startup); startup.flags=0x100;
            startup.stdin=inR; startup.stdout=outW; startup.stderr=errW;
            Check(CreateProcessW(executable,new StringBuilder(raw),IntPtr.Zero,IntPtr.Zero,true,0x08000000,IntPtr.Zero,null,ref startup,out process));
            Close(ref process.thread); Close(ref outW); Close(ref errW); Close(ref inR);
            using(var output=new FileStream(new SafeFileHandle(outR,true),FileAccess.Read,4096,false))
            using(var error=new FileStream(new SafeFileHandle(errR,true),FileAccess.Read,4096,false))
            using(var stdout=new MemoryStream())
            using(var stderr=new MemoryStream()) {
                outR=IntPtr.Zero; errR=IntPtr.Zero;
                IntPtr inputHandle=inW; inW=IntPtr.Zero;
                Task inTask=Task.Run(() => {
                    using (var input=new FileStream(new SafeFileHandle(inputHandle,true),FileAccess.Write,4096,false)) {
                        try {input.Write(inputBytes,0,inputBytes.Length);} catch(IOException e) {if ((e.HResult & 65535)!=109 && (e.HResult & 65535)!=232) throw;}
                    }
                });
                Task outTask=output.CopyToAsync(stdout), errTask=error.CopyToAsync(stderr);
                uint wait=WaitForSingleObject(process.process,(uint)timeoutMs);
                bool completed=wait==0;
                if (wait==258) { Check(TerminateProcess(process.process,1460)); if (WaitForSingleObject(process.process,5000)!=0) throw new TimeoutException("Child would not terminate"); }
                else if (!completed) throw new Win32Exception(Marshal.GetLastWin32Error());
                if (!Task.WaitAll(new Task[]{outTask,errTask,inTask},5000)) throw new TimeoutException("Inherited output handles remain open");
                uint exit; Check(GetExitCodeProcess(process.process,out exit)); timer.Stop();
                return new WmicCaptureResult {
                    arguments_utf16_base64=WideBase64(arguments), command_line_utf16_base64=WideBase64(raw),
                    state=completed ? "completed" : "timeout", exit_status=completed ? (uint?)exit : null,
                    duration_ms=timer.ElapsedMilliseconds, process_id=process.processId,
                    stdout_base64=Convert.ToBase64String(stdout.ToArray()), stderr_base64=Convert.ToBase64String(stderr.ToArray())
                };
            }
        } finally {
            Close(ref outR); Close(ref outW); Close(ref errR); Close(ref errW); Close(ref inR); Close(ref inW);
            Close(ref process.thread); Close(ref process.process);
        }
    }
}
'@
    $request = $ManifestJson | ConvertFrom-Json
    $cases = @(foreach ($case in $request.cases) {
        if ($case.operation -ne 'read_only') { throw 'This capture mode accepts reviewed read-only cases only.' }
        $arguments = $case.arguments
        if ($case.arguments_utf16_base64) { $arguments = [WmicReferenceCapture]::DecodeWide($case.arguments_utf16_base64) }
        if ($arguments -match '(?i)/password\s*:') { throw 'Credential arguments cannot be recorded.' }
        $timeout = 20000
        if ($case.timeout_ms) { $timeout = [int]$case.timeout_ms }
        $inputBytes=[byte[]]@()
        if ($case.stdin_base64) {$inputBytes=[Convert]::FromBase64String($case.stdin_base64)}
        $result = [WmicReferenceCapture]::RunWithInput($file.FullName, $arguments, $timeout, $inputBytes)
        $result.id = $case.id
        $result.normalizations = @()
        if ($case.normalizations) { $result.normalizations = @($case.normalizations) }
        $result
    })
    $profile = [ordered]@{
        role = $Role; executable = $file.FullName; executable_sha256 = $hash
        version = $file.VersionInfo.FileVersion; signature = $signature.Status.ToString()
        computer = $env:COMPUTERNAME; identity = [Security.Principal.WindowsIdentity]::GetCurrent().Name
        launcher_architecture = $env:PROCESSOR_ARCHITECTURE
        culture = [Globalization.CultureInfo]::CurrentCulture.Name
        ui_culture = [Globalization.CultureInfo]::CurrentUICulture.Name
        ansi_codepage = [WmicReferenceCapture]::GetACP(); oem_codepage = [WmicReferenceCapture]::GetOEMCP()
        console_output_codepage = [WmicReferenceCapture]::GetConsoleOutputCP()
        directory = [Environment]::CurrentDirectory; launch_api = 'CreateProcessW'; output_mode = 'native pipes'
    }
    $json = [ordered]@{ schema_version = 1; profile = $profile; cases = $cases } | ConvertTo-Json -Depth 10 -Compress
    [Convert]::ToBase64String([Text.Encoding]::UTF8.GetBytes($json))
} catch {
    [Console]::Error.WriteLine($_.Exception.ToString())
    exit 1
}
