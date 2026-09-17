; Acceptance as an act rather than the absence of one. Defined here because
; this file is included before the pages are built; /S skips pages anyway.
!define MUI_LICENSEPAGE_CHECKBOX
!define MUI_LICENSEPAGE_CHECKBOX_TEXT "I have read and accept the licence and the terms above"

; What an uninstall leaves behind, said on the page where it still matters.
;
; A silo is a folder the user chose, holding everything that makes it a
; silo, and removing the app must never remove it: an uninstall that took
; the data with it would be unrecoverable for anyone without a backup. So
; the silos stay, deliberately, and the page says so rather than letting
; someone find out either way afterwards.
;
; The box below this text is Tauri's own. It removes the list of silos this
; computer keeps, the WebView2 profile, and (see the post-uninstall hook)
; the machine-local working directory. It does not touch a silo.
;
; Two lines, because that is what the page gives: this label is a fixed
; static control, and Tauri's checkbox is drawn at a hard-coded offset under
; it. The full list of what stays is in README.md under "Uninstalling".
!define MUI_UNCONFIRMPAGE_TEXT_TOP "Your silos stay where they are: uninstalling never deletes silo data, here or in your backup storage. The app and its Explorer menu entries go."

; Autostart must not outlive the install: Windows keeps running a Run entry
; whose exe is gone, and the user gets an error box at every sign-in with no
; obvious way to trace it back to an app they removed.
;
; HKCU is the right hive here because the installer is per-user (Tauri's
; default install mode), so the uninstaller runs as the same user that owns
; the entry. The marker file goes too: its absence is what tells a fresh
; install that this machine has never been asked about autostart, so a
; reinstall behaves like a first install instead of staying silently off.
;
; The Explorer verbs go with it, for the same reason: "Add to SilentSilo" on
; a right-click menu after the app is gone runs nothing and cannot be traced
; back to anything the user can see. The app writes these at every start
; (`silentsilo-shell::ensure_os_integration`), keyed off
; shell-integration.json, so an update that removes them here has them back
; the moment the new version runs. The four bases match
; `silentsilo-shell::windows::unregister_context_menu`: change one and
; change the other.
;
; The two queue files hold plaintext paths of whatever was last right-clicked
; and are never cleaned up otherwise, so they go regardless of the box.
!macro NSIS_HOOK_PREUNINSTALL
  DeleteRegValue HKCU "Software\Microsoft\Windows\CurrentVersion\Run" "SilentSilo"
  Delete "$LOCALAPPDATA\SilentSilo\autostart-initialized"

  DeleteRegKey HKCU "Software\Classes\*\shell\SilentSiloUpload"
  DeleteRegKey HKCU "Software\Classes\Directory\shell\SilentSiloUpload"
  DeleteRegKey HKCU "Software\Classes\Directory\Background\shell\SilentSiloDownload"
  DeleteRegKey HKCU "Software\Classes\DesktopBackground\shell\SilentSiloDownload"

  Delete "$LOCALAPPDATA\SilentSilo\shell-integration.json"
  Delete "$LOCALAPPDATA\SilentSilo\upload-queue.txt"
  Delete "$LOCALAPPDATA\SilentSilo\download-queue.txt"
!macroend

; The rest of the machine-local directory, under the same box that removes
; the silo list. Tauri's own block clears $APPDATA\${BUNDLEID} and
; $LOCALAPPDATA\${BUNDLEID}; the working copies are not in either, because
; they are written by the domain crates under a name of their own.
;
; Only under the box, and never mid-update. A working copy holds everything
; since the last lock when a session ended in a crash, so deleting one
; uninvited could take records the snapshot does not have yet.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $DeleteAppDataCheckboxState = 1
  ${AndIf} $UpdateMode <> 1
    SetShellVarContext current
    RMDir /r "$LOCALAPPDATA\SilentSilo"
  ${EndIf}
!macroend
