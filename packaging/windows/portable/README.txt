UniClipboard — Portable (green) build
UniClipboard — 便携版（绿色版）
=====================================

English
-------
This is the portable build of UniClipboard. It needs no installation and keeps
all of your data next to the executable.

How to use:
  1. Keep UniClipboard.exe and portable.dat together in the same folder.
  2. Double-click UniClipboard.exe to run.

Where your data lives:
  All data (encrypted database, keys, search index, logs) is stored in a
  "data" folder created next to UniClipboard.exe. Delete that folder to reset
  the app. Move the whole folder (exe + portable.dat + data) to carry your
  setup to another machine or a USB stick.

The "portable.dat" marker is what enables portable mode. If you remove it, the
app reverts to storing data under %LOCALAPPDATA%\app.uniclipboard.desktop\
like the installed version.

Updates:
  The portable build does NOT update itself. To upgrade, download the newer
  "*-portable.zip" from the Releases page and replace UniClipboard.exe. Your
  "data" folder is preserved across upgrades.
  Releases: https://github.com/UniClipboard/UniClipboard/releases

Requirements:
  Windows 10/11 with the Microsoft Edge WebView2 Runtime (preinstalled on
  current Windows). The x64 zip runs on Intel/AMD; the arm64 zip runs on
  Windows on ARM devices.

Known limitation:
  The WebView2 runtime's browser cache is still written under %LOCALAPPDATA%
  (it is not redirected into the portable folder yet), so a portable run is not
  100% trace-free. Your actual data (database, keys, search index, logs) does
  live in the "data" folder next to the exe.


中文
----
这是 UniClipboard 的便携版（绿色版），免安装，所有数据都放在程序旁边。

使用方法：
  1. 让 UniClipboard.exe 与 portable.dat 保持在同一个文件夹内。
  2. 双击 UniClipboard.exe 运行。

数据存放位置：
  所有数据（加密数据库、密钥、搜索索引、日志）都保存在 UniClipboard.exe
  同目录下自动创建的 “data” 文件夹中。删除该文件夹即可重置应用。把整个
  文件夹（exe + portable.dat + data）一起拷走，即可迁移到其它电脑或 U 盘。

“portable.dat” 标记文件用于开启便携模式。删除它后，应用会和安装版一样把
数据写到 %LOCALAPPDATA%\app.uniclipboard.desktop\。

更新：
  便携版不会自更新。升级时请从 Releases 页面下载新的 “*-portable.zip” 并
  替换 UniClipboard.exe，升级过程中 “data” 文件夹会被保留。
  Releases: https://github.com/UniClipboard/UniClipboard/releases

运行要求：
  Windows 10/11，并安装 Microsoft Edge WebView2 Runtime（新版 Windows 已
  预装）。x64 版用于 Intel/AMD 设备，arm64 版用于 Windows on ARM 设备。

已知限制：
  WebView2 运行时的浏览器缓存目前仍写在 %LOCALAPPDATA%（尚未重定向进便携
  文件夹），因此便携运行并非 100% 零痕迹。但你真正的数据（数据库、密钥、
  搜索索引、日志）都在 exe 旁边的 “data” 文件夹里。
