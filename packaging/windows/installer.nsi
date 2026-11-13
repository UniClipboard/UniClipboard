Unicode true
!include "LogicLib.nsh"
!include "WordFunc.nsh"
!insertmacro VersionCompare
Name "UniClipboard"
OutFile "@OUTPUT@"
InstallDir "$LOCALAPPDATA\UniClipboard"
InstallDirRegKey HKCU "Software\UniClipboard" "InstallDir"
RequestExecutionLevel user
VIProductVersion "@VERSION@.@BUILD@"
VIAddVersionKey "ProductName" "UniClipboard"
VIAddVersionKey "FileDescription" "UniClipboard installer"
VIAddVersionKey "FileVersion" "@VERSION@.@BUILD@"
Function .onInit
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "InstallLocation"
  ${If} $R0 != ""
    StrCpy $R1 $R0 1
    ${If} $R1 == '$\"'
      StrCpy $R0 $R0 -1 1
    ${EndIf}
    StrCpy $INSTDIR $R0
  ${EndIf}
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "DisplayVersion"
  ${If} $R0 != ""
    ${VersionCompare} "$R0" "@VERSION@.@BUILD@" $R1
    ${If} $R1 == 1
      MessageBox MB_ICONSTOP "A newer UniClipboard version is installed."
      Abort
    ${EndIf}
  ${EndIf}
FunctionEnd
Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles
Section
  ; Require existing v1/v2 executables to be closed before changing files.
  !macro CheckClosed NAME
    ${If} ${FileExists} "$INSTDIR\${NAME}"
    System::Call 'kernel32::CreateFileW(w "$INSTDIR\${NAME}", i 0x40000000, i 0, p 0, i 3, i 0, p 0) p.r0'
    ${If} $0 == -1
      MessageBox MB_ICONSTOP "Quit UniClipboard and stop its service before changing this installation."
      SetErrorLevel 2
      Abort
    ${EndIf}
    System::Call 'kernel32::CloseHandle(p r0)'
    ${EndIf}
  !macroend
  !insertmacro CheckClosed "unic-desktop.exe"
  !insertmacro CheckClosed "unicd.exe"
  !insertmacro CheckClosed "unic.exe"
  !insertmacro CheckClosed "UniClipboard.exe"
  !insertmacro CheckClosed "uniclipd.exe"
  !insertmacro CheckClosed "uniclip-quick-panel.exe"
  SetOutPath "$INSTDIR"
  File "@PAYLOAD@/unic-desktop.exe"
  File "@PAYLOAD@/unic.exe"
  File "@PAYLOAD@/unicd.exe"
  File "@PAYLOAD@/uniclip.cmd"
  File "@PAYLOAD@/installation.json"
  File "@PAYLOAD@/LICENSE"
  Delete "$INSTDIR\UniClipboard.exe"
  Delete "$INSTDIR\uniclipd.exe"
  Delete "$INSTDIR\uniclip-quick-panel.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\UniClipboard.lnk" "$INSTDIR\unic-desktop.exe"
  WriteRegStr HKCU "Software\UniClipboard" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "DisplayName" "UniClipboard"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "DisplayVersion" "@VERSION@.@BUILD@"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
SectionEnd
Section "Uninstall"
  !insertmacro CheckClosed "unic-desktop.exe"
  !insertmacro CheckClosed "unicd.exe"
  !insertmacro CheckClosed "unic.exe"
  Delete "$INSTDIR\unic-desktop.exe"
  Delete "$INSTDIR\unic.exe"
  Delete "$INSTDIR\unicd.exe"
  Delete "$INSTDIR\uniclip.cmd"
  Delete "$INSTDIR\installation.json"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$SMPROGRAMS\UniClipboard.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard"
  DeleteRegKey HKCU "Software\UniClipboard"
  RMDir "$INSTDIR"
SectionEnd
