Unicode true
!include "LogicLib.nsh"
!include "WordFunc.nsh"
!include "FileFunc.nsh"
Var UpdateOwner
Var LegacyUpdate
Var LegacyRestart
!insertmacro VersionCompare
Name "UniClipboard"
Icon "@PAYLOAD@/icon.ico"
UninstallIcon "@PAYLOAD@/icon.ico"
OutFile "@OUTPUT@"
InstallDir "$LOCALAPPDATA\UniClipboard"
InstallDirRegKey HKCU "Software\UniClipboard" "InstallDir"
RequestExecutionLevel user
VIProductVersion "@VERSION@.@BUILD@"
VIAddVersionKey "ProductName" "UniClipboard"
VIAddVersionKey "FileDescription" "UniClipboard installer"
VIAddVersionKey "FileVersion" "@VERSION@.@BUILD@"
Function .onInit
  StrCpy $UpdateOwner "direct"
  ${GetParameters} $R2
  ; v1 sends the restart arguments after /ARGS. v2 starts with its own defaults.
  StrCpy $R3 $R2 19
  StrCpy $R4 $R2 1 19
  ${If} $R3 == "/P /R /UPDATE /ARGS"
    ${If} $R4 == ""
    ${OrIf} $R4 == " "
      StrCpy $LegacyUpdate 1
      StrCpy $LegacyRestart 1
    ${EndIf}
  ${ElseIf} $R2 == "/P /UPDATE /ARGS"
    StrCpy $LegacyUpdate 1
  ${EndIf}
  ${If} $LegacyUpdate == 1
    SetSilent silent
  ${Else}
    ${GetOptions} $R2 "/MANAGED=" $R3
    ${If} $R3 == "winget"
      StrCpy $UpdateOwner "winget"
    ${ElseIf} $R3 == "chocolatey"
      StrCpy $UpdateOwner "chocolatey"
    ${EndIf}
  ${EndIf}
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "InstallLocation"
  ${If} $R0 != ""
    StrCpy $R1 $R0 1
    ${If} $R1 == '$\"'
      StrCpy $R0 $R0 -1 1
    ${EndIf}
    StrCpy $INSTDIR $R0
  ${ElseIf} $LegacyUpdate == 1
    SetErrorLevel 2
    Abort
  ${EndIf}
  ReadRegStr $R0 HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "DisplayVersion"
  ${If} $LegacyUpdate == 1
    ${If} $R0 != "1.6.2-alpha.2"
    ${OrIfNot} ${FileExists} "$INSTDIR\UniClipboard.exe"
      SetErrorLevel 2
      Abort
    ${EndIf}
  ${EndIf}
  ${If} $R0 != ""
    ${VersionCompare} "$R0" "@VERSION@.@BUILD@" $R1
    ${If} $R1 == 1
      MessageBox MB_ICONSTOP "A newer UniClipboard version is installed." /SD IDOK
      SetErrorLevel 2
      Abort
    ${EndIf}
  ${EndIf}
FunctionEnd
Page directory
Page instfiles
UninstPage uninstConfirm
UninstPage instfiles
!macro CheckClosed NAME WAIT
  ${If} ${FileExists} "$INSTDIR\${NAME}"
    StrCpy $1 0
    ${Do}
      System::Call 'kernel32::CreateFileW(w "$INSTDIR\${NAME}", i 0x40000000, i 0, p 0, i 3, i 0, p 0) p.r0'
      ${If} $0 != -1
        System::Call 'kernel32::CloseHandle(p r0)'
        ${ExitDo}
      ${EndIf}
      ${If} ${WAIT} != 1
      ${OrIf} $1 >= 40
        MessageBox MB_ICONSTOP "Quit UniClipboard and stop its service before changing this installation." /SD IDOK
        SetErrorLevel 2
        Abort
      ${EndIf}
      Sleep 250
      IntOp $1 $1 + 1
    ${Loop}
  ${EndIf}
!macroend
Section
  !insertmacro CheckClosed "unic-desktop.exe" 0
  !insertmacro CheckClosed "unicd.exe" 0
  !insertmacro CheckClosed "unic.exe" 0
  !insertmacro CheckClosed "UniClipboard.exe" $LegacyUpdate
  !insertmacro CheckClosed "uniclipd.exe" $LegacyUpdate
  !insertmacro CheckClosed "uniclip-quick-panel.exe" $LegacyUpdate
  SetOutPath "$INSTDIR"
  File "@PAYLOAD@/unic-desktop.exe"
  File "@PAYLOAD@/unic.exe"
  File "@PAYLOAD@/unicd.exe"
  File "@PAYLOAD@/uniclip.cmd"
  File "@PAYLOAD@/installation.json"
  File "@PAYLOAD@/icon.ico"
  File "@PAYLOAD@/LICENSE"
  File "@PAYLOAD@/release-identity.json"
  File "@PAYLOAD@/.mygo-files"
  ${If} $UpdateOwner == "direct"
    ClearErrors
    FileOpen $R4 "$INSTDIR\update-owner" r
    IfErrors owner_write
    FileRead $R4 $UpdateOwner
    FileClose $R4
  ${EndIf}
  owner_write:
  FileOpen $R4 "$INSTDIR\update-owner" w
  FileWrite $R4 $UpdateOwner
  FileClose $R4
  Delete "$INSTDIR\UniClipboard.exe"
  Delete "$INSTDIR\uniclipd.exe"
  Delete "$INSTDIR\uniclip-quick-panel.exe"
  WriteUninstaller "$INSTDIR\uninstall.exe"
  CreateShortcut "$SMPROGRAMS\UniClipboard.lnk" "$INSTDIR\unic-desktop.exe"
  ${If} $LegacyUpdate == 1
  ${AndIf} ${FileExists} "$DESKTOP\UniClipboard.lnk"
    CreateShortcut "$DESKTOP\UniClipboard.lnk" "$INSTDIR\unic-desktop.exe"
  ${EndIf}
  WriteRegStr HKCU "Software\UniClipboard" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "InstallLocation" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "DisplayName" "UniClipboard"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "DisplayVersion" "@VERSION@.@BUILD@"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "UninstallString" '$\"$INSTDIR\uninstall.exe$\"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard" "QuietUninstallString" '$\"$INSTDIR\uninstall.exe$\" /S'
SectionEnd
Function .onInstSuccess
  ${If} $LegacyRestart == 1
    Exec '"$INSTDIR\unic-desktop.exe"'
  ${EndIf}
FunctionEnd
Section "Uninstall"
  !insertmacro CheckClosed "unic-desktop.exe" 0
  !insertmacro CheckClosed "unicd.exe" 0
  !insertmacro CheckClosed "unic.exe" 0
  Delete "$INSTDIR\unic-desktop.exe"
  Delete "$INSTDIR\unic.exe"
  Delete "$INSTDIR\unicd.exe"
  Delete "$INSTDIR\uniclip.cmd"
  Delete "$INSTDIR\installation.json"
  Delete "$INSTDIR\icon.ico"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\release-identity.json"
  Delete "$INSTDIR\.mygo-files"
  Delete "$INSTDIR\update-owner"
  Delete "$INSTDIR\uninstall.exe"
  Delete "$SMPROGRAMS\UniClipboard.lnk"
  Delete "$DESKTOP\UniClipboard.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\UniClipboard"
  DeleteRegKey HKCU "Software\UniClipboard"
  RMDir "$INSTDIR"
SectionEnd
