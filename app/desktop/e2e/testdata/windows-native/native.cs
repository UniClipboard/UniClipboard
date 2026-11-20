using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.Runtime.InteropServices;
using System.Text;

public static class UnicNativeB973 {
    [DllImport("dwmapi.dll")] static extern int DwmFlush();
    public static void FlushCompositor() { int result=DwmFlush();if(result!=0)Marshal.ThrowExceptionForHR(result); }
    [StructLayout(LayoutKind.Sequential)] public struct NOTIFYICONIDENTIFIER { public uint cbSize; public IntPtr hWnd; public uint uID; public Guid guidItem; }
    public sealed class TrayRectObservation { public int ProcessId; public uint IconId; public long Hwnd; public int HResult; public int Left,Top,Right,Bottom; public uint IdentifierSize; }
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr hwnd,StringBuilder value,int max);
    [DllImport("shell32.dll",SetLastError=true)] static extern int Shell_NotifyIconGetRect(ref NOTIFYICONIDENTIFIER identifier,out RECT value);
    public sealed class ExplorerRootObservation { public long Hwnd; public string ClassName; public int Left,Top,Right,Bottom; public bool Visible; }
    // Read-only inventory of one already verified Explorer PID. Never read titles.
    public static ExplorerRootObservation[] ExplorerRoots(int explorerProcess) {
        System.Collections.Generic.List<ExplorerRootObservation> list=new System.Collections.Generic.List<ExplorerRootObservation>();
        EnumWindows(delegate(IntPtr hwnd,IntPtr parameter) {
            if(Owner(hwnd)!=explorerProcess)return true;
            StringBuilder className=new StringBuilder(256);GetClassName(hwnd,className,className.Capacity);
            RECT rectangle;GetWindowRect(hwnd,out rectangle);
            ExplorerRootObservation item=new ExplorerRootObservation();item.Hwnd=hwnd.ToInt64();item.ClassName=className.ToString();item.Left=rectangle.Left;item.Top=rectangle.Top;item.Right=rectangle.Right;item.Bottom=rectangle.Bottom;item.Visible=IsWindowVisible(hwnd);list.Add(item);
            return true;
        },IntPtr.Zero);
        return list.ToArray();
    }
    public sealed class OwnedWindowObservation { public long Hwnd; public string Title; public string ClassName; public int Left,Top,Right,Bottom; public bool Visible; }
    // Titles are read only after ownership matches the verified task role PID.
    public static OwnedWindowObservation[] OwnedWindows(int taskProcess) {
        System.Collections.Generic.List<OwnedWindowObservation> list=new System.Collections.Generic.List<OwnedWindowObservation>();
        EnumWindows(delegate(IntPtr hwnd,IntPtr parameter) {
            if(Owner(hwnd)!=taskProcess)return true;
            StringBuilder className=new StringBuilder(256);GetClassName(hwnd,className,className.Capacity);
            StringBuilder title=new StringBuilder(1024);GetWindowText(hwnd,title,title.Capacity);
            RECT rectangle;GetWindowRect(hwnd,out rectangle);
            OwnedWindowObservation item=new OwnedWindowObservation();item.Hwnd=hwnd.ToInt64();item.Title=title.ToString();item.ClassName=className.ToString();item.Left=rectangle.Left;item.Top=rectangle.Top;item.Right=rectangle.Right;item.Bottom=rectangle.Bottom;item.Visible=IsWindowVisible(hwnd);list.Add(item);
            return true;
        },IntPtr.Zero);
        return list.ToArray();
    }
    // Read only. Do not expose or change the icon or notification-area settings.
    public static TrayRectObservation TrayRect(int process,uint iconId) {
        System.Collections.Generic.List<IntPtr> windows=new System.Collections.Generic.List<IntPtr>();
        EnumWindows(delegate(IntPtr hwnd,IntPtr parameter) {
            if(Owner(hwnd)!=process)return true;
            StringBuilder name=new StringBuilder(256);GetClassName(hwnd,name,name.Capacity);
            if(name.ToString()=="MyGoApp")windows.Add(hwnd);
            return true;
        },IntPtr.Zero);
        if(windows.Count!=1)throw new Exception("Expected one MyGoApp HWND for task PID; found "+windows.Count);
        NOTIFYICONIDENTIFIER identifier=new NOTIFYICONIDENTIFIER();identifier.cbSize=(uint)Marshal.SizeOf(typeof(NOTIFYICONIDENTIFIER));identifier.hWnd=windows[0];identifier.uID=iconId;identifier.guidItem=Guid.Empty;
        RECT rectangle;int result=Shell_NotifyIconGetRect(ref identifier,out rectangle);
        TrayRectObservation observation=new TrayRectObservation();observation.ProcessId=process;observation.IconId=iconId;observation.Hwnd=windows[0].ToInt64();observation.HResult=result;observation.Left=rectangle.Left;observation.Top=rectangle.Top;observation.Right=rectangle.Right;observation.Bottom=rectangle.Bottom;observation.IdentifierSize=identifier.cbSize;
        return observation;
    }

    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern uint GetDpiForSystem();
    [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr hwnd);
    [DllImport("user32.dll")] static extern short GetAsyncKeyState(int key);
    public static bool MouseButtonsIdle() { return (GetAsyncKeyState(0x01)&0x8000)==0&&(GetAsyncKeyState(0x02)&0x8000)==0; }
    public static void CheckModifiers() {
        foreach(int key in new int[]{0x10,0x11,0x12,0x5B,0x5C,0x56}) {
            if((GetAsyncKeyState(key)&0x8000)!=0)throw new Exception("A user modifier or V is held; no hotkey injected");
        }
    }
    [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left,Top,Right,Bottom; }
    [StructLayout(LayoutKind.Sequential)] public struct MOUSEINPUT { public int dx,dy; public uint mouseData,dwFlags,time; public UIntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Sequential)] public struct KEYBDINPUT { public ushort wVk,wScan; public uint dwFlags,time; public UIntPtr dwExtraInfo; }
    [StructLayout(LayoutKind.Explicit)] public struct INPUTUNION { [FieldOffset(0)] public MOUSEINPUT mi; [FieldOffset(0)] public KEYBDINPUT ki; }
    [StructLayout(LayoutKind.Sequential)] public struct INPUT { public uint type; public INPUTUNION data; }
    [DllImport("user32.dll",SetLastError=true)] static extern uint SendInput(uint count,INPUT[] values,int size);
    [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
    [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hwnd,out uint process);
    [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hwnd);
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr hwnd,out RECT rect);
    [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr hwnd,StringBuilder title,int max);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hwnd);
    [DllImport("user32.dll",SetLastError=true)] public static extern bool PostMessage(IntPtr hwnd,uint message,IntPtr wParam,IntPtr lParam);
    [DllImport("user32.dll")] static extern bool EnumWindows(EnumWindowProc callback,IntPtr parameter);
    delegate bool EnumWindowProc(IntPtr hwnd,IntPtr parameter);
    [DllImport("user32.dll")] static extern bool PrintWindow(IntPtr hwnd,IntPtr hdc,uint flags);
    public static int Owner(IntPtr hwnd) { uint value;GetWindowThreadProcessId(hwnd,out value);return (int)value; }
    public static IntPtr[] Windows(int process,string title) {
        System.Collections.Generic.List<IntPtr> list=new System.Collections.Generic.List<IntPtr>();
        EnumWindows(delegate(IntPtr hwnd,IntPtr parameter) { if(Owner(hwnd)!=process||!IsWindowVisible(hwnd))return true;StringBuilder text=new StringBuilder(1024);GetWindowText(hwnd,text,text.Capacity);if(text.ToString()==title)list.Add(hwnd);return true;},IntPtr.Zero);
        return list.ToArray();
    }
    public static void Mouse(int x,int y,bool right) {
        int button=right?0x02:0x01;
        if((GetAsyncKeyState(button)&0x8000)!=0)throw new Exception("User mouse button is held; no mouse action injected");
        if(!SetCursorPos(x,y))throw new Exception("SetCursorPos failed");
        INPUT[] values=new INPUT[2];values[0].type=0;values[1].type=0;
        values[0].data.mi.dwFlags=right?0x0008u:0x0002u;values[1].data.mi.dwFlags=right?0x0010u:0x0004u;
        uint sent=SendInput(2,values,Marshal.SizeOf(typeof(INPUT)));
        if(sent!=2){
            uint released=0;
            if(sent==1){INPUT[] cleanup=new INPUT[1];cleanup[0]=values[1];released=SendInput(1,cleanup,Marshal.SizeOf(typeof(INPUT)));}
            throw new Exception("Mouse partial/unknown outcome; inserted="+sent+"/2, own_button_releases="+released+"; click not replayed");
        }
    }
    public static void Hotkey() {
        CheckModifiers();
        ushort[] keys=new ushort[]{0x11,0x12,0x56,0x56,0x12,0x11};INPUT[] values=new INPUT[6];
        for(int index=0;index<6;index++){values[index].type=1;values[index].data.ki.wVk=keys[index];values[index].data.ki.dwFlags=index<3?0u:2u;}
        uint sent=SendInput(6,values,Marshal.SizeOf(typeof(INPUT)));
        if(sent!=6){
            int released=0;
            // Only release modifiers this call could have pressed. Never replay V.
            foreach(ushort key in new ushort[]{0x12,0x11}) {
                if((key==0x11&&sent>=1||key==0x12&&sent>=2)&&(GetAsyncKeyState(key)&0x8000)!=0){
                    INPUT[] cleanup=new INPUT[1];cleanup[0].type=1;cleanup[0].data.ki.wVk=key;cleanup[0].data.ki.dwFlags=2;
                    released+=(int)SendInput(1,cleanup,Marshal.SizeOf(typeof(INPUT)));
                }
            }
            throw new Exception("Hotkey partial/unknown outcome; inserted="+sent+"/6, modifier_releases="+released+"; business key not replayed");
        }
    }
    public static string Capture(IntPtr hwnd,int expectedProcess,string file) {
        if(Owner(hwnd)!=expectedProcess)throw new Exception("Capture owner changed");RECT rect;if(!GetWindowRect(hwnd,out rect)||rect.Right<=rect.Left||rect.Bottom<=rect.Top)throw new Exception("Capture rectangle unavailable");
        if(Owner(GetForegroundWindow())!=expectedProcess)throw new Exception("Capture requires the verified task process in foreground");
        using(Bitmap bitmap=new Bitmap(rect.Right-rect.Left,rect.Bottom-rect.Top))using(Graphics graphics=Graphics.FromImage(bitmap)){
            if(Owner(hwnd)!=expectedProcess)throw new Exception("Capture owner changed");
            graphics.CopyFromScreen(rect.Left,rect.Top,0,0,bitmap.Size);
            bitmap.Save(file,ImageFormat.Png);return "CopyFromScreen cropped to verified foreground owned window/menu";
        }
    }
}
