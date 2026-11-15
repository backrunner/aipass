; AIPass native installer, maintained in this repository.
Unicode true
!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"
Name "AIPass"
OutFile "${OUTPUT}"
InstallDir "$LOCALAPPDATA\AIPass"
InstallDirRegKey HKCU "Software\AIPass" "InstallDir"
RequestExecutionLevel user
SetCompressor /SOLID lzma
!define MUI_ICON "${PAYLOAD}\icon.ico"
!define MUI_UNICON "${PAYLOAD}\icon.ico"
!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"
!insertmacro MUI_LANGUAGE "SimpChinese"
!macro RegisterScheme SCHEME
  WriteRegStr HKCU "Software\Classes\${SCHEME}" "" "URL:AIPass"
  WriteRegStr HKCU "Software\Classes\${SCHEME}" "URL Protocol" ""
  WriteRegStr HKCU "Software\Classes\${SCHEME}\shell\open\command" "" '$"$INSTDIR\aipass-desktop.exe$" $"%1$"'
!macroend
!macro RemoveScheme SCHEME
  ReadRegStr $R1 HKCU "Software\Classes\${SCHEME}\shell\open\command" ""
  ${If} $R1 == '$"$INSTDIR\aipass-desktop.exe$" $"%1$"'
    DeleteRegKey HKCU "Software\Classes\${SCHEME}"
  ${EndIf}
!macroend
Section "Install"
  ; Manual replacement must drain the old desktop and Agent before copying.
  ClearErrors
  ${GetOptions} $CMDLINE "/UPDATE" $R0
  ${If} ${Errors}
    IfFileExists "$INSTDIR\aipass-desktop.exe" 0 install_files
    nsExec::ExecToLog '"$INSTDIR\aipass-desktop.exe" --remove-login-start'
    Pop $R0
    ${If} $R0 != 0
      SetErrorLevel 2
      Abort "Close AIPass before replacing this installation."
    ${EndIf}
  ${EndIf}
  install_files:
  SetOutPath "$INSTDIR"
  File /r "${PAYLOAD}\*"
  WriteUninstaller "$INSTDIR\Uninstall.exe"
  WriteRegStr HKCU "Software\AIPass" "InstallDir" "$INSTDIR"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AIPass" "DisplayName" "AIPass"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AIPass" "DisplayVersion" "${VERSION}"
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AIPass" "UninstallString" '$"$INSTDIR\Uninstall.exe$"'
  WriteRegStr HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AIPass" "DisplayIcon" "$INSTDIR\icon.ico"
  CreateShortcut "$SMPROGRAMS\AIPass.lnk" "$INSTDIR\aipass-desktop.exe"
  !insertmacro RegisterScheme "aipass"
  !insertmacro RegisterScheme "ccswitch"
  !insertmacro RegisterScheme "aipass-provider"
SectionEnd
Section "Uninstall"
  nsExec::ExecToLog '"$INSTDIR\aipass-agent.exe" --prepare-uninstall'
  Pop $R0
  ${If} $R0 != 0
    SetErrorLevel 2
    Abort "Could not restore tool configurations. Recovery data was retained."
  ${EndIf}
  nsExec::ExecToLog '"$INSTDIR\aipass-desktop.exe" --remove-login-start'
  Pop $R0
  ${If} $R0 != 0
    SetErrorLevel 2
    Abort "Could not stop AIPass. Close it and retry."
  ${EndIf}
  nsExec::ExecToLog '"$INSTDIR\aipass-agent.exe" --uninstall-autostart'
  Pop $R0
  ${If} $R0 != 0
    SetErrorLevel 2
    Abort "Could not stop Agent."
  ${EndIf}
  !insertmacro RemoveScheme "aipass"
  !insertmacro RemoveScheme "ccswitch"
  !insertmacro RemoveScheme "aipass-provider"
  Delete "$SMPROGRAMS\AIPass.lnk"
  DeleteRegKey HKCU "Software\Microsoft\Windows\CurrentVersion\Uninstall\AIPass"
  DeleteRegKey HKCU "Software\AIPass"
  ; Only known installed payloads; user vault and preferences remain intact.
  Delete "$INSTDIR\aipass*.exe"
  Delete "$INSTDIR\icon.ico"
  Delete "$INSTDIR\desktop.json"
  Delete "$INSTDIR\Uninstall.exe"
  RMDir /r "$INSTDIR\browser-extension"
  RMDir /r "$INSTDIR\licenses"
  RMDir "$INSTDIR"
SectionEnd
