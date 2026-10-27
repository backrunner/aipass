!include "LogicLib.nsh"
!include "FileFunc.nsh"

!macro AIPASS_RESTORE_PROXY AGENT
  ClearErrors
  nsExec::ExecToLog '"${AGENT}" --prepare-uninstall'
  Pop $R0
  ${If} $R0 != 0
    DetailPrint "Could not safely restore Agent configurations. Recovery data was retained. Resolve the conflict and retry uninstall."
    IfSilent +2
      MessageBox MB_OK|MB_ICONSTOP "Could not safely restore Agent configurations. Uninstall was stopped. Resolve the configuration conflict and retry."
    SetErrorLevel 2
    Abort
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ; Replacing an installation must never detach its proxy configuration.
  ; /AIPASS_REPLACE is also passed by manual upgrade/downgrade installers.
  ${If} $UpdateMode != 1
    ClearErrors
    ${GetOptions} $CMDLINE "/AIPASS_REPLACE" $R9
    ${If} ${Errors}
      IfFileExists "$INSTDIR\aipass-agent.exe" 0 aipass_restore_resource_agent
        !insertmacro AIPASS_RESTORE_PROXY "$INSTDIR\aipass-agent.exe"
        Goto aipass_proxy_restore_done
      aipass_restore_resource_agent:
      IfFileExists "$INSTDIR\resources\aipass-agent.exe" 0 aipass_restore_missing_agent
        !insertmacro AIPASS_RESTORE_PROXY "$INSTDIR\resources\aipass-agent.exe"
        Goto aipass_proxy_restore_done
      aipass_restore_missing_agent:
        DetailPrint "The Agent recovery executable is missing. Repair AIPass before uninstalling."
        SetErrorLevel 2
        Abort
    ${EndIf}
  ${EndIf}
  aipass_proxy_restore_done:
  DetailPrint "Removing AIPass tray login entries..."
  IfFileExists "$INSTDIR\aipass-desktop.exe" 0 aipass_tray_cleanup_done
    nsExec::ExecToLog '"$INSTDIR\aipass-desktop.exe" --remove-login-start'
    Pop $R0
    ${If} $R0 != 0
      DetailPrint "Could not stop AIPass or remove login entries. Close AIPass and retry."
      SetErrorLevel 2
      Abort
    ${EndIf}
  aipass_tray_cleanup_done:
  DetailPrint "Removing AIPass agent autostart..."
  IfFileExists "$INSTDIR\aipass-agent.exe" 0 aipass_agent_cleanup_done
    nsExec::ExecToLog '"$INSTDIR\aipass-agent.exe" --uninstall-autostart'
    Pop $R0
    ${If} $R0 != 0
      DetailPrint "Could not stop the AIPass Agent. Close AIPass and retry."
      SetErrorLevel 2
      Abort
    ${EndIf}
  aipass_agent_cleanup_done:
  IfFileExists "$INSTDIR\resources\aipass-agent.exe" 0 aipass_resource_agent_cleanup_done
    nsExec::ExecToLog '"$INSTDIR\resources\aipass-agent.exe" --uninstall-autostart'
    Pop $R0
    ${If} $R0 != 0
      SetErrorLevel 2
      Abort
    ${EndIf}
  aipass_resource_agent_cleanup_done:
!macroend
