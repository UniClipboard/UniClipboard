using System;
using System.Collections.Generic;
using System.Runtime.InteropServices;
using System.Text;
public static class MemoryWindowState {
 public class Item {public long Handle;public string Title;public bool Visible;public bool Foreground;}
 delegate bool Callback(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] static extern bool EnumWindows(Callback c,IntPtr p);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 public static Item[] ForProcess(uint pid){
  var result=new List<Item>();var foreground=GetForegroundWindow();
  Callback c=delegate(IntPtr h,IntPtr p){uint owner;GetWindowThreadProcessId(h,out owner);if(owner==pid){var title=new StringBuilder(256);GetWindowText(h,title,256);result.Add(new Item{Handle=h.ToInt64(),Title=title.ToString(),Visible=IsWindowVisible(h),Foreground=h==foreground});}return true;};
  EnumWindows(c,IntPtr.Zero);GC.KeepAlive(c);return result.ToArray();
 }
}
