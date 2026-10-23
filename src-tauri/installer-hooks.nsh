; NSIS hooks for the Tauri installer (bundle.windows.nsis.installerHooks in tauri.conf.json).

; Uninstalling removes what launch at login leaves for this user. Tauri's own uninstall section
; already deletes the Run value (HKCU ...\CurrentVersion\Run\${PRODUCTNAME}), but not the
; StartupApproved\Run value that the autostart plugin writes beside it. Skipped on an update
; (the old version's uninstaller run with /UPDATE), so launch at login survives one, as Tauri
; does for the Run value. Deleting a value that isn't there does nothing. The data folder
; (%LOCALAPPDATA%\ProductivityTracker) stays, as documented.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode <> 1
    DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run" "${PRODUCTNAME}"
  ${EndIf}
!macroend
