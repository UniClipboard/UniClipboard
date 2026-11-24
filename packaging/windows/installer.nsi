Unicode true
!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "WordFunc.nsh"
!include "FileFunc.nsh"

Var UpdateOwner
Var LegacyUpdate
Var LegacyRestart
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
VIAddVersionKey "LegalCopyright" "UniClipboard Contributors"

!define MUI_ICON "@PAYLOAD@/icon.ico"
!define MUI_UNICON "@PAYLOAD@/icon.ico"
!define MUI_ABORTWARNING

!insertmacro MUI_PAGE_WELCOME
!define MUI_PAGE_CUSTOMFUNCTION_LEAVE CheckAndStopBeforeInstall
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_NOREBOOTSUPPORT
!define MUI_FINISHPAGE_RUN "$INSTDIR\unic-desktop.exe"
!define MUI_FINISHPAGE_RUN_TEXT "$(LaunchAppText)"
!insertmacro MUI_PAGE_FINISH

!define MUI_PAGE_CUSTOMFUNCTION_LEAVE un.CheckAndStopBeforeUninstall
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!define MUI_FINISHPAGE_NOREBOOTSUPPORT
!insertmacro MUI_UNPAGE_FINISH

!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"
!insertmacro MUI_LANGUAGE "TradChinese"

LangString MsgNewerInstalled ${LANG_ENGLISH} "A newer version of UniClipboard is already installed."
LangString MsgNewerInstalled ${LANG_SIMPCHINESE} "已安装更新版本的 UniClipboard。"
LangString MsgNewerInstalled ${LANG_TRADCHINESE} "已安裝較新版本的 UniClipboard。"

LangString MsgRunningInstall ${LANG_ENGLISH} "UniClipboard or its background service is currently running.$\r$\n$\r$\nDo you want to stop it and continue installing?"
LangString MsgRunningInstall ${LANG_SIMPCHINESE} "UniClipboard 或其后台服务正在运行。$\r$\n$\r$\n是否停止并继续安装？"
LangString MsgRunningInstall ${LANG_TRADCHINESE} "UniClipboard 或其背景服務正在執行。$\r$\n$\r$\n是否停止並繼續安裝？"

LangString MsgRunningUninstall ${LANG_ENGLISH} "UniClipboard or its background service is currently running.$\r$\n$\r$\nDo you want to stop it and continue uninstalling?"
LangString MsgRunningUninstall ${LANG_SIMPCHINESE} "UniClipboard 或其后台服务正在运行。$\r$\n$\r$\n是否停止并继续卸载？"
LangString MsgRunningUninstall ${LANG_TRADCHINESE} "UniClipboard 或其背景服務正在執行。$\r$\n$\r$\n是否停止並繼續解除安裝？"

LangString MsgStillRunning ${LANG_ENGLISH} "UniClipboard or its background service is still running and could not be stopped automatically.$\r$\n$\r$\nPlease quit UniClipboard manually, then click Retry."
LangString MsgStillRunning ${LANG_SIMPCHINESE} "UniClipboard 或其后台服务仍在运行，无法自动停止。$\r$\n$\r$\n请手动退出 UniClipboard，然后点击“重试”。"
LangString MsgStillRunning ${LANG_TRADCHINESE} "UniClipboard 或其背景服務仍在執行，無法自動停止。$\r$\n$\r$\n請手動結束 UniClipboard，然後按一下「重試」。"

LangString MsgQuitBeforeChange ${LANG_ENGLISH} "Quit UniClipboard and stop its service before changing this installation."
LangString MsgQuitBeforeChange ${LANG_SIMPCHINESE} "更改此安装前，请先退出 UniClipboard 并停止其后台服务。"
LangString MsgQuitBeforeChange ${LANG_TRADCHINESE} "變更此安裝前，請先結束 UniClipboard 並停止其背景服務。"

LangString LaunchAppText ${LANG_ENGLISH} "&Launch UniClipboard"
LangString LaunchAppText ${LANG_SIMPCHINESE} "启动 UniClipboard(&L)"
LangString LaunchAppText ${LANG_TRADCHINESE} "啟動 UniClipboard(&L)"

!macro InitUILanguage
  System::Call 'kernel32::GetUserDefaultUILanguage() i.r0'
  ${If} $0 == 3076 ; zh-HK
  ${OrIf} $0 == 5124 ; zh-MO
    StrCpy $LANGUAGE ${LANG_TRADCHINESE}
  ${EndIf}
!macroend

!macro CheckFileLocked NAME
  ${If} ${FileExists} "$INSTDIR\${NAME}"
    StrCpy $R0 "$INSTDIR\${NAME}"
    System::Call 'kernel32::CreateFileW(w R0, i 0x40000000, i 0, p 0, i 3, i 0, p 0) p.r0'
    ${If} $0 != -1
      System::Call 'kernel32::CloseHandle(p r0)'
    ${Else}
      StrCpy $R1 1
    ${EndIf}
  ${EndIf}
!macroend

!macro CheckClosed NAME WAIT
  ${If} ${FileExists} "$INSTDIR\${NAME}"
    StrCpy $R0 "$INSTDIR\${NAME}"
    StrCpy $1 0
    ${Do}
      System::Call 'kernel32::CreateFileW(w R0, i 0x40000000, i 0, p 0, i 3, i 0, p 0) p.r0'
      ${If} $0 != -1
        System::Call 'kernel32::CloseHandle(p r0)'
        ${ExitDo}
      ${EndIf}
      ${If} ${WAIT} != 1
      ${OrIf} $1 >= 40
        MessageBox MB_ICONSTOP "$(MsgQuitBeforeChange)" /SD IDOK
        SetErrorLevel 2
        Abort
      ${EndIf}
      Sleep 250
      IntOp $1 $1 + 1
    ${Loop}
  ${EndIf}
!macroend

!macro DefineProcessHelpers UN INCLUDE_LEGACY PROMPT_STR
Function ${UN}HasLockedFiles
  System::Store "S"
  StrCpy $R1 0
  !insertmacro CheckFileLocked "unic-desktop.exe"
  !insertmacro CheckFileLocked "unicd.exe"
  !insertmacro CheckFileLocked "unic.exe"
  !if ${INCLUDE_LEGACY} == 1
    !insertmacro CheckFileLocked "UniClipboard.exe"
    !insertmacro CheckFileLocked "uniclipd.exe"
    !insertmacro CheckFileLocked "uniclip-quick-panel.exe"
  !endif
  Push $R1
  System::Store "L"
FunctionEnd

Function ${UN}StopRunningProcesses
  System::Store "S"
  System::Call 'kernel32::GetCurrentProcessId() i.R9'
  System::Call 'kernel32::GetFullPathNameW(w d, i ${NSIS_MAX_STRLEN}, w .R5, p 0) i.r0'
  ${If} $0 == 0
  ${OrIf} $0 >= ${NSIS_MAX_STRLEN}
    StrCpy $R5 "$INSTDIR"
  ${EndIf}
  System::Call 'kernel32::GetLongPathNameW(w R5, w .R4, i ${NSIS_MAX_STRLEN}) i.r0'
  ${If} $0 > 0
  ${AndIf} $0 < ${NSIS_MAX_STRLEN}
    StrCpy $R5 "$R4"
  ${EndIf}
  StrCpy $0 $R5 1 -1
  ${If} $0 == "\"
    StrCpy $R5 $R5 -1
  ${EndIf}
  System::Alloc 65536
  Pop $R8
  ${If} $R8 != 0
    StrCpy $R3 0
    ${Do}
      System::Call 'kernel32::K32EnumProcesses(p R8, i 65536, *i 0 R7) i.r0'
      ${If} $0 != 0
      ${AndIf} $R7 > 0
        StrCpy $R6 0
        ${DoWhile} $R6 < $R7
          IntOp $1 $R8 + $R6
          System::Call '*$1(i.r2)'
          ${If} $2 != 0
          ${AndIf} $2 != $R9
            System::Call 'kernel32::OpenProcess(i 0x1000, i 0, i r2) p.r3'
            ${If} $3 != 0
              StrCpy $4 ""
              System::Call 'kernel32::QueryFullProcessImageNameW(p r3, i 0, w .r4, *i ${NSIS_MAX_STRLEN} .) i.r6'
              System::Call 'kernel32::CloseHandle(p r3)'
              ${If} $6 != 0
                StrCpy $7 0
                ${If} $4 == "$R5\unic-desktop.exe"
                ${OrIf} $4 == "$R5\unicd.exe"
                ${OrIf} $4 == "$R5\unic.exe"
                  StrCpy $7 1
                ${EndIf}
                !if ${INCLUDE_LEGACY} == 1
                  ${If} $4 == "$R5\UniClipboard.exe"
                  ${OrIf} $4 == "$R5\uniclipd.exe"
                  ${OrIf} $4 == "$R5\uniclip-quick-panel.exe"
                    StrCpy $7 1
                  ${EndIf}
                !endif
                ${If} $7 == 1
                  System::Call 'kernel32::OpenProcess(i 0x00100001, i 0, i r2) p.r3'
                  ${If} $3 != 0
                    System::Call 'kernel32::TerminateProcess(p r3, i 0) i.r6'
                    System::Call 'kernel32::WaitForSingleObject(p r3, i 3000) i.r6'
                    System::Call 'kernel32::CloseHandle(p r3)'
                  ${EndIf}
                ${EndIf}
              ${EndIf}
            ${EndIf}
          ${EndIf}
          IntOp $R6 $R6 + 4
        ${Loop}
      ${EndIf}
      StrCpy $R2 0
      ${Do}
        Call ${UN}HasLockedFiles
        Pop $0
        ${If} $0 == 0
          ${ExitDo}
        ${EndIf}
        ${If} $R2 >= 10
          ${ExitDo}
        ${EndIf}
        Sleep 250
        IntOp $R2 $R2 + 1
      ${Loop}
      ${If} $0 == 0
        ${ExitDo}
      ${EndIf}
      IntOp $R3 $R3 + 1
    ${LoopWhile} $R3 < 3
    System::Free $R8
  ${EndIf}
  System::Store "L"
FunctionEnd

Function ${UN}PromptAndStopRunning
  Call ${UN}HasLockedFiles
  Pop $0
  ${If} $0 == 0
    Push 1
    Return
  ${EndIf}
  MessageBox MB_ICONQUESTION|MB_YESNO|MB_DEFBUTTON1 "$(${PROMPT_STR})" IDYES stop_loop
  Push 0
  Return
stop_loop:
  Call ${UN}StopRunningProcesses
  Call ${UN}HasLockedFiles
  Pop $0
  ${If} $0 == 0
    Push 1
    Return
  ${EndIf}
  MessageBox MB_ICONEXCLAMATION|MB_RETRYCANCEL "$(MsgStillRunning)" IDRETRY stop_loop
  Push 0
FunctionEnd
!macroend

!insertmacro DefineProcessHelpers "" 1 "MsgRunningInstall"
!insertmacro DefineProcessHelpers "un." 0 "MsgRunningUninstall"

Function CheckAndStopBeforeInstall
  Call PromptAndStopRunning
  Pop $0
  ${If} $0 == 0
    Abort
  ${EndIf}
FunctionEnd

Function un.CheckAndStopBeforeUninstall
  Call un.PromptAndStopRunning
  Pop $0
  ${If} $0 == 0
    Abort
  ${EndIf}
FunctionEnd

Function .onInit
  !insertmacro InitUILanguage
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
    ${AndIf} $R0 != "1.6.2"
      SetErrorLevel 2
      Abort
    ${EndIf}
    ${IfNot} ${FileExists} "$INSTDIR\UniClipboard.exe"
      SetErrorLevel 2
      Abort
    ${EndIf}
  ${EndIf}
  ${If} $R0 != ""
    ${VersionCompare} "$R0" "@VERSION@.@BUILD@" $R1
    ${If} $R1 == 1
      MessageBox MB_ICONSTOP "$(MsgNewerInstalled)" /SD IDOK
      SetErrorLevel 2
      Abort
    ${EndIf}
  ${EndIf}
FunctionEnd

Function un.onInit
  !insertmacro InitUILanguage
FunctionEnd

Section
  ${If} ${Silent}
    !insertmacro CheckClosed "unic-desktop.exe" 0
    !insertmacro CheckClosed "unicd.exe" 0
    !insertmacro CheckClosed "unic.exe" 0
    !insertmacro CheckClosed "UniClipboard.exe" $LegacyUpdate
    !insertmacro CheckClosed "uniclipd.exe" $LegacyUpdate
    !insertmacro CheckClosed "uniclip-quick-panel.exe" $LegacyUpdate
  ${Else}
    Call PromptAndStopRunning
    Pop $0
    ${If} $0 == 0
      SetErrorLevel 2
      Abort
    ${EndIf}
  ${EndIf}
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
  ${If} ${Silent}
    !insertmacro CheckClosed "unic-desktop.exe" 0
    !insertmacro CheckClosed "unicd.exe" 0
    !insertmacro CheckClosed "unic.exe" 0
  ${Else}
    Call un.PromptAndStopRunning
    Pop $0
    ${If} $0 == 0
      SetErrorLevel 2
      Abort
    ${EndIf}
  ${EndIf}
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
