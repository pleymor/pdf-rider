; NSIS installer hooks (bundle.windows.nsis.installerHooks in tauri.conf.json).
;
; Tauri's file association registers the "PDF Document" ProgID with an "open"
; verb only. Add Explorer's "Print" verb, usable on a multiple selection:
; MultiSelectModel=Player lifts the 15-file limit; Explorer runs the command
; once per file and the processes merge into one print session.
; The uninstaller deletes the whole ProgID key (APP_UNASSOCIATE), print included.

!macro NSIS_HOOK_POSTINSTALL
  WriteRegStr SHCTX "Software\Classes\PDF Document\shell\print" "MultiSelectModel" "Player"
  WriteRegStr SHCTX "Software\Classes\PDF Document\shell\print\command" "" "$\"$INSTDIR\${MAINBINARYNAME}.exe$\" --print $\"%1$\""
  ; Tell Explorer the association changed (SHCNE_ASSOCCHANGED)
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
!macroend
