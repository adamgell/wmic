// Runs inside its own hidden console. It never attaches to an existing console.
using System;
using System.IO;
using System.Text;
using System.Runtime.InteropServices;
using System.Threading;
public static class WmicConsoleProbe {
 [StructLayout(LayoutKind.Sequential)] struct COORD {public short X,Y;public COORD(short x,short y){X=x;Y=y;}}
 [StructLayout(LayoutKind.Sequential)] struct RECT {public short L,T,R,B;}
 [StructLayout(LayoutKind.Sequential)] struct INFO {public COORD Size,Cursor;public short Attributes;public RECT Window;public COORD Maximum;}
 [StructLayout(LayoutKind.Sequential,CharSet=CharSet.Unicode)] struct STARTUP {public int cb;public string reserved,desktop,title;public int x,y,width,height,xChars,yChars,fill,flags;public short show,reserved2;public IntPtr reservedPtr,input,output,error;}
 [StructLayout(LayoutKind.Sequential)] struct PROCESS {public IntPtr process,thread;public uint pid,tid;}
 [StructLayout(LayoutKind.Explicit,Size=20)] struct INPUT {
  [FieldOffset(0)]public short Type;[FieldOffset(4)]public int Down;[FieldOffset(8)]public short Repeat;
  [FieldOffset(10)]public short VirtualKey;[FieldOffset(12)]public short Scan;[FieldOffset(14)]public char Character;[FieldOffset(16)]public uint Control;
 }
 delegate bool Ctrl(uint code);
 static Ctrl ignore=delegate(uint code){return code==0||code==1;};
 [DllImport("kernel32",SetLastError=true)]static extern IntPtr GetStdHandle(int n);
 [DllImport("kernel32",SetLastError=true)]static extern bool SetConsoleScreenBufferSize(IntPtr h,COORD c);
 [DllImport("kernel32",SetLastError=true)]static extern bool GetConsoleScreenBufferInfo(IntPtr h,out INFO info);
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool ReadConsoleOutputCharacterW(IntPtr h,[Out]char[] b,uint n,COORD pos,out uint read);
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool WriteConsoleInputW(IntPtr h,INPUT[] data,uint count,out uint written);
 [DllImport("kernel32",SetLastError=true)]static extern bool SetConsoleCtrlHandler(Ctrl handler,bool add);
 [DllImport("kernel32",SetLastError=true)]static extern bool GenerateConsoleCtrlEvent(uint code,uint group);
 [DllImport("kernel32",CharSet=CharSet.Unicode,SetLastError=true)]static extern bool CreateProcessW(string exe,StringBuilder command,IntPtr pa,IntPtr ta,bool inherit,uint flags,IntPtr env,string cwd,ref STARTUP startup,out PROCESS process);
 [DllImport("kernel32",SetLastError=true)]static extern uint WaitForSingleObject(IntPtr h,uint ms);
 [DllImport("kernel32",SetLastError=true)]static extern bool GetExitCodeProcess(IntPtr h,out uint status);
 [DllImport("kernel32",SetLastError=true)]static extern bool TerminateProcess(IntPtr h,uint status);
 [DllImport("kernel32")]static extern bool CloseHandle(IntPtr h);
 static string Screen(IntPtr h,out INFO info){if(!GetConsoleScreenBufferInfo(h,out info))throw new Exception("No private console: "+Marshal.GetLastWin32Error());char[] b=new char[(info.Cursor.Y+1)*info.Size.X];uint count;if(!ReadConsoleOutputCharacterW(h,b,(uint)b.Length,new COORD(0,0),out count))throw new Exception("Read screen failed");return new string(b,0,(int)count);}
 public static int Main(string[] args){
  // exe, arguments, UTF16 input base64, report, mode
  PROCESS child=new PROCESS();bool started=false;IntPtr output=GetStdHandle(-11);
  try{
   if(!SetConsoleScreenBufferSize(output,new COORD(120,3000)))throw new Exception("Set private console buffer failed: "+Marshal.GetLastWin32Error());
   if(!SetConsoleCtrlHandler(ignore,true))throw new Exception("Private console handler failed");
   STARTUP si=new STARTUP();si.cb=Marshal.SizeOf(typeof(STARTUP));
   if(!CreateProcessW(args[0],new StringBuilder("\""+args[0]+"\" "+args[1]),IntPtr.Zero,IntPtr.Zero,false,0,IntPtr.Zero,Environment.CurrentDirectory,ref si,out child))throw new Exception("Child start failed: "+Marshal.GetLastWin32Error());started=true;
   INFO info;var until=DateTime.UtcNow.AddSeconds(10);string screen="";
   string awaited=args[4].StartsWith("cancel")||args[4]=="break-key"?"Microsoft Windows":"wmic:";
   int expected=args[4]=="cancel-second"?2:1;
   while(DateTime.UtcNow<until){screen=Screen(output,out info);if(screen.Split(new[]{awaited},StringSplitOptions.None).Length-1>=expected||WaitForSingleObject(child.process,0)==0)break;Thread.Sleep(20);}
   if(args[4].StartsWith("cancel")){
    if(!screen.Contains(awaited))throw new Exception("No first result before cancellation");
    if(!GenerateConsoleCtrlEvent(0,0))throw new Exception("Private console cancellation failed");
   }else if(args[4]=="typeahead"){
    var input=Encoding.Unicode.GetString(Convert.FromBase64String(args[2]));var events=new INPUT[input.Length];
    for(int i=0;i<input.Length;i++){events[i].Type=1;events[i].Down=1;events[i].Repeat=1;events[i].Character=input[i];events[i].VirtualKey=(short)(input[i]=='\r'?13:0);}
    uint written;if(!WriteConsoleInputW(GetStdHandle(-10),events,(uint)events.Length,out written))throw new Exception("Typeahead input failed");
   }else if(args[4]=="break-key"){
    var key=new INPUT[1];key[0].Type=1;key[0].Down=1;key[0].Repeat=1;key[0].Character='x';uint written;
    if(!WriteConsoleInputW(GetStdHandle(-10),key,1,out written))throw new Exception("Break key failed");
   }else if(args[4]!="noinput"){
    var text=Encoding.Unicode.GetString(Convert.FromBase64String(args[2]));
    int prompts=1;
    foreach(var line in text.Split(new[]{'\r'},StringSplitOptions.None)){
     if(line.Length==0 && prompts>text.Split('\r').Length-1)break;
     var input=line+"\r";var events=new INPUT[input.Length];
     for(int i=0;i<input.Length;i++){events[i].Type=1;events[i].Down=1;events[i].Repeat=1;events[i].Character=input[i];events[i].VirtualKey=(short)(input[i]=='\r'?13:0);}
     uint written;if(!WriteConsoleInputW(GetStdHandle(-10),events,(uint)events.Length,out written)||written!=events.Length)throw new Exception("Private console input failed");
     prompts++;until=DateTime.UtcNow.AddSeconds(10);
     while(DateTime.UtcNow<until){screen=Screen(output,out info);if(screen.Split(new[]{"wmic:"},StringSplitOptions.None).Length-1>=prompts||WaitForSingleObject(child.process,0)==0)break;Thread.Sleep(20);}
     if(WaitForSingleObject(child.process,0)==0)break;
    }
   }
   var state=WaitForSingleObject(child.process,10000)==0?"completed":"timeout";
   if(state=="timeout"){TerminateProcess(child.process,0xFFFFFFFF);WaitForSingleObject(child.process,2000);}
   uint code;GetExitCodeProcess(child.process,out code);Thread.Sleep(100);screen=Screen(output,out info);
   File.WriteAllLines(args[3],new[]{state,code.ToString(),Convert.ToBase64String(Encoding.Unicode.GetBytes(screen)),info.Size.X.ToString(),info.Cursor.X.ToString(),info.Cursor.Y.ToString()},new UTF8Encoding(false));return 0;
  }catch(Exception e){if(started){TerminateProcess(child.process,0xFFFFFFFF);WaitForSingleObject(child.process,2000);}File.WriteAllLines(args[3],new[]{"error",e.ToString()});return 1;}
  finally{if(child.thread!=IntPtr.Zero)CloseHandle(child.thread);if(child.process!=IntPtr.Zero)CloseHandle(child.process);}
 }
}
