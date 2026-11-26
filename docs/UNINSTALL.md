# Uninstalling SilentSilo

A silo is a folder you chose, and uninstalling never touches it. Nothing in
your backup storage is touched either. What the uninstaller does remove,
whatever you answer on its one question:

- the application itself, from its install folder
- the Explorer menu entries (*Add to SilentSilo*, *Save here from SilentSilo*)
- the sign-in entry that starts it, so Windows stops trying to run it
- the browser extension's registration: its host, the host's two manifests,
  and the keys named `com.silentsilo.desktop` under
  `HKCU\Software\Google\Chrome\NativeMessagingHosts` (read by Chrome and
  Brave), `HKCU\Software\Microsoft\Edge\NativeMessagingHosts` and
  `HKCU\Software\Mozilla\NativeMessagingHosts` (Firefox)
- `%LOCALAPPDATA%\SilentSilo\upload-queue.txt` and `download-queue.txt`,
  which hold the paths of whatever was last right-clicked

Ticking **Delete the application data** also removes:

- `%APPDATA%\com.silentsilo.desktop`, the list of silos this computer keeps
  (names and folder paths) and the auto-type settings. Losing it costs the list, not the data: the app
  finds a silo again from its folder.
- `%LOCALAPPDATA%\SilentSilo`, the encrypted working copies, the cache and
  the browser extension setting
- `%LOCALAPPDATA%\com.silentsilo.desktop`, WebView2's profile for this app

Two things stay behind in either case, and have to be removed by hand if you
want them gone:

- **Your silo folders.** Deleting one deletes that silo, and no copy of the
  files is kept anywhere else unless you set one up. There is no undo.
- **Windows Credential Manager** keeps the storage credentials you saved,
  and the device secret of a silo that has no key yet (a key opening it
  empties that one). Open *Credential Manager* → *Windows Credentials* and
  remove the entries ending in `.com.silentsilo.desktop`. Do it after the
  silo folders, not before: a silo with no key does not open without its
  device secret.

## On Linux

Remove the .deb with your package manager, or delete the AppImage.
Neither touches your silo folders or your backup storage. What the app
leaves in your home, to remove by hand if you want it gone:

- `~/.local/share/com.silentsilo.desktop`, the list of silos (names and
  folder paths), and `~/.local/share/SilentSilo`, the app's settings and,
  for the AppImage, the copy of the browser host
- `~/.cache/silentsilo`, the encrypted working copies, the cache and, on a
  system without a keyring, the secret files you agreed to keep there
- `~/.config/autostart/com.silentsilo.desktop.desktop`, if it starts at
  login
- the browser extension's manifests, `com.silentsilo.desktop.json` under
  each browser's `NativeMessagingHosts` (or Firefox's
  `native-messaging-hosts`). Turning the extension off in Settings before
  uninstalling removes them.
- storage credentials, and the device secret of a silo with no key yet, in
  your keyring (GNOME Keyring or KWallet, through the Secret Service), under
  names ending in `.com.silentsilo.desktop`. As on Windows, remove them
  after the silo folders, not before.

## On macOS

Quit SilentSilo and move it from Applications to the Trash. That does not
touch your silo folders or your backup storage. What the app leaves in your
home, to remove by hand if you want it gone:

- `~/Library/Application Support/com.silentsilo.desktop`, the list of silos
  (names and folder paths)
- `~/Library/Application Support/SilentSilo`, the app's settings and its
  copy of the browser host, and `~/.cache/silentsilo`, the encrypted working
  copies
- `~/Library/Caches/com.silentsilo.desktop` and
  `~/Library/WebKit/com.silentsilo.desktop`, the window's caches
- `~/Library/LaunchAgents/com.silentsilo.desktop.plist`, if it starts at
  login
- the browser extension's manifests, `com.silentsilo.desktop.json` under
  each browser's `NativeMessagingHosts` in `~/Library/Application Support`.
  Turning the extension off in Settings before deleting the app removes
  them.
- storage credentials, Touch ID keys and the device secret of a silo with
  no key yet, in your login keychain, under names ending in
  `.com.silentsilo.desktop`. As on Windows, remove them after the silo
  folders, not before.
