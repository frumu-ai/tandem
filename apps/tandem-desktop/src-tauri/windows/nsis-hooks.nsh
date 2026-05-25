!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Stopping running Tandem processes before install..."
  nsExec::ExecToLog 'taskkill /F /T /IM tandem-engine.exe'
  nsExec::ExecToLog 'taskkill /F /T /IM tandem.exe'
  nsExec::ExecToLog 'taskkill /F /T /IM Tandem.exe'
  Sleep 1200
!macroend

