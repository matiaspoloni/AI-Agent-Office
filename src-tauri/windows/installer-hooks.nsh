; Agent Office additions to Tauri's NSIS installer (bundle > windows > nsis >
; installerHooks). The macros run inside Tauri's install/uninstall sections;
; $UpdateMode and $DeleteAppDataCheckboxState come from Tauri's template.

!macro NSIS_HOOK_POSTINSTALL
  ; Installing a new version first runs the old uninstaller, which removed the
  ; hook integrations and noted which ones: put those back. Only a note from
  ; the last hour counts, so a later separate install adds nothing by itself.
  ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" integrations restore' $0
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    ; Close Agent Office first (the same check Tauri runs next), so that
    ; cancelling here leaves everything as it was.
    !insertmacro CheckIfAppIsRunning "$INSTDIR\${MAINBINARYNAME}.exe" "${PRODUCTNAME}"
    ; Claude Code and Codex CLI must not keep calling a program that is about
    ; to be removed: take out Agent Office's hook entries (only ours; the
    ; settings files are backed up first) and note which were present.
    ExecWait '"$INSTDIR\${MAINBINARYNAME}.exe" integrations uninstall --remember' $0
  ${EndIf}
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; "Delete the application data" also removes Agent Office's own folder
  ; (database, logs, IPC token); Tauri's step only covers the WebView data.
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    RmDir /r "$LOCALAPPDATA\AgentOffice"
  ${EndIf}
!macroend
