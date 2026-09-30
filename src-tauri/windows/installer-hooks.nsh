; Only NSIS installs receive this marker. Portable archives contain the EXE only.
!macro NSIS_HOOK_POSTINSTALL
  FileOpen $0 "$INSTDIR\.remova-install-channel" w
  FileWrite $0 "nsis-current-user-v1"
  FileClose $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  Delete "$INSTDIR\.remova-install-channel"
!macroend
