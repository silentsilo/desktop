; Autostart must not outlive the install: Windows keeps running a Run entry
; whose exe is gone, and the user gets an error box at every sign-in with no
; obvious way to trace it back to an app they removed.
;
; HKCU is the right hive here because the installer is per-user (Tauri's
; default install mode), so the uninstaller runs as the same user that owns
; the entry. The marker file goes too: its absence is what tells a fresh
; install that this machine has never been asked about autostart, so a
; reinstall behaves like a first install instead of staying silently off.
!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SilentSilo"
  Delete "$LOCALAPPDATA\SilentSilo\autostart-initialized"
!macroend
