using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public static class MemoryNative {
 [DllImport("user32.dll")] public static extern bool OpenClipboard(IntPtr h);
 [DllImport("user32.dll")] public static extern bool CloseClipboard();
 [DllImport("user32.dll")] public static extern uint GetClipboardSequenceNumber();
 [DllImport("user32.dll")] public static extern uint GetGuiResources(IntPtr h,uint f);
 [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int cmd);
 [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h,uint msg,IntPtr w,IntPtr l);
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int w,int height,uint flags);
 [DllImport("kernel32.dll")] public static extern bool IsWow64Process2(IntPtr h,out ushort process,out ushort native);
 [StructLayout(LayoutKind.Sequential)] public struct Rect {public int Left,Top,Right,Bottom;}
 [StructLayout(LayoutKind.Sequential)] public struct MBI {public ulong BaseAddress,AllocationBase;public uint AllocationProtect,PartitionId;public ulong RegionSize;public uint State,Protect,Type;}
 [DllImport("kernel32.dll")] static extern UIntPtr VirtualQueryEx(IntPtr h,UIntPtr a,out MBI b,UIntPtr len);
 [DllImport("psapi.dll",SetLastError=true)] static extern bool QueryWorkingSet(IntPtr h,IntPtr p,int n);
 public static Dictionary<string,ulong> Resident(IntPtr h) {
  int size=16*1024*1024; var p=Marshal.AllocHGlobal(size);
  try {
   if(!QueryWorkingSet(h,p,size)) throw new Exception("QueryWorkingSet: "+Marshal.GetLastWin32Error());
   ulong n=(ulong)Marshal.ReadInt64(p); ulong shared=0,priv=0;
   for(ulong i=0;i<n;i++){ulong f=(ulong)Marshal.ReadInt64(p,checked((int)((i+1)*8))); if((f&256)!=0)shared+=4096;else priv+=4096;}
   return new Dictionary<string,ulong>{{"privateWorkingSet",priv},{"sharedWorkingSet",shared},{"workingSetPages",n}};
  } finally {Marshal.FreeHGlobal(p);}
 }
 public static Dictionary<string,ulong> Regions(IntPtr h) {
  var d=new Dictionary<string,ulong>{{"privateCommitted",0},{"mappedCommitted",0},{"imageCommitted",0},{"reserved",0}};
  ulong addr=0; MBI b;
  while(VirtualQueryEx(h,(UIntPtr)addr,out b,(UIntPtr)Marshal.SizeOf(typeof(MBI)))!=UIntPtr.Zero){
   if(b.State==0x1000){string k=b.Type==0x20000?"privateCommitted":b.Type==0x40000?"mappedCommitted":"imageCommitted";d[k]+=b.RegionSize;}
   else if(b.State==0x2000)d["reserved"]+=b.RegionSize;
   ulong next=b.BaseAddress+b.RegionSize;if(next<=addr)break;addr=next;
  } return d;
 }
}
