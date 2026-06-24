!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing AIPass agent autostart..."
  IfFileExists "$INSTDIR\aipass-agent.exe" 0 +2
    nsExec::ExecToLog '"$INSTDIR\aipass-agent.exe" --uninstall-autostart'
  IfFileExists "$INSTDIR\resources\aipass-agent.exe" 0 +2
    nsExec::ExecToLog '"$INSTDIR\resources\aipass-agent.exe" --uninstall-autostart'
!macroend
