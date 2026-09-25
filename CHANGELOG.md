# Changelog

Notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). From this release
onward the version follows semver, and anything that could stop an existing
silo from opening needs a major version rather than a note.

## [Unreleased]

Update every computer and phone that uses a silo before replacing its
encryption key. A device still on 1.1 can keep writing under the old key to a
never-delete copy and hold up the others until it updates. A never-delete copy
itself keeps the old key after a replacement and gets no new backups: remove
it under Backup, which leaves what is stored there, and add a new one. Until
then it shows as not backed up, and it no longer holds up the others.

### Added

- Browser extension support, off by default under Settings > Browser
  extension. The SilentSilo extension for Chrome, Edge, Brave and Firefox
  can ask for the logins saved for the site in the open tab and fill one,
  after you confirm in this window with Windows Hello or your security key. It sees login
  entries only, never files, notes or codes. The installer registers a small
  native messaging host for it; uninstalling removes the registration. The
  app talks only to that host, signed like the app, and the host only to
  those four browsers and to this app. Brave uses Chrome's registration;
  a browser whose extension listing does not exist yet is not registered.
  Firefox is registered only once the Firefox Add-ons listing holds the
  extension's id, so no one else can publish an add-on under it first.
  Filling needs a security key or Windows Hello on the silo. After a
  confirmed fill, a window that was hidden or minimised goes back there.
- The extension's popup has an "Open SilentSilo" button when the silo is
  locked. It brings this window to the front, where you unlock as usual;
  then click the extension again. The extension cannot unlock anything.
- Settings > Browser extension lists the fills since SilentSilo started:
  site, login and time. One you do not recognise means something else on
  this computer asked for it. Searching from the extension is limited to a
  few queries a minute.

### Changed

- Syncing runs the same code as the Android app, from the shared core, so
  one set of tests covers both. The desktop kept its own copy until now.
- Shorter, plainer texts across the app. Most hints are now a sentence or
  two, and technical terms on screen are replaced with everyday words.
- One set of names across the app. Credentials is now Passwords, with one
  saved item called an entry, and Security keys is now Keys. The trash
  offers Move to trash, Delete for good and Empty trash, the status bar
  says Synced or Sync failed, and a key rotation is called Replace the
  encryption key. Screens say backup storage, this computer and never-delete
  copy, in British spelling.
- Settings opens on an Overview of what keeps the silo safe: backup,
  copies, recovery code, the printed kit, a key you can carry and the last
  backup test, each with the action that fixes it. Backup and Copies are one
  page, Verification is Test backup, Keys and Auto-lock are Unlocking, and
  Devices and Activity are one page. Replacing the encryption key, turning
  off the recovery code and removing the silo moved to Advanced.
- General, Browser extension and Updates and about belong to the app and
  open from the silo list and the unlock screen too. The default auto-lock
  can be changed there, and the theme can follow the system.
- A new silo walks through its recovery code and backup storage right after
  its first key, and either can be left for later.
- Choosing backup storage starts on a drive or NAS folder, with the other
  kinds named in plain words.
- Opening another silo starts on its files, not on the page the last one was
  left on.
- Protected folders are now called Auto-import folders, which says what they
  do: files from them are copied into the silo each time it unlocks, and
  deleting one on the computer leaves the silo's copy alone.
- The Backup page has a Test backup button and says when the backup was last
  tested on this computer.

### Fixed

- A file edited and then deleted for good before the next sync could leave
  another computer with a copy of that edit it could not open. Emptying the
  trash now keeps content no backup holds yet until the next sync has sent
  it.
- Photos a phone sends land in the same folder on every computer, also when
  the Phone folder had been deleted. Two computers importing at once could
  each keep them in a folder of their own.
- Changes made on a computer that was offline for more than a month now
  reach the other computers. If one of them compacted the history right
  after they arrived, the others could miss them for good.
- A file added to a folder while another computer emptied that folder from
  the trash now shows on every computer, including one set up again from
  backup storage afterwards, which used to drop it.
- Rebuilding a silo after it fell behind no longer brings back old changes.
  With a backup copy that was not reachable, such as an unplugged drive, a
  rebuild wrote old history again as new changes: folders emptied from the
  trash came back and older edits could replace newer ones.
- After replacing the encryption key, photos sent from a phone leave the
  inbox again, and old deleted content is cleaned up again. A never-delete
  copy still under the old key held both up.
- A folder import no longer stops half way at a subfolder it cannot read;
  it skips it and carries on. Setting up a silo from backup storage that
  fails part way can be tried again at once, and refuses a folder that is
  not empty, as creating a silo does.
- A silo's idle lock no longer clears a password copied from another silo,
  and a key change checks the key you touch before changing anything. A
  backup copy that missed a key change can no longer put the old key back
  on the others.
- A key change can no longer leave a silo that nothing opens. The keys and
  the recovery code are now saved together with the new key; a crash or a
  locked file between them used to strand it.
- A key change stops before it starts when a backup storage cannot be
  opened, instead of skipping it and leaving the old key working there. A
  damaged record no longer blocks a key change for good.
- A backup drive that is not plugged in shows as unreachable, not as an
  empty copy, and syncing no longer creates its folder again and fills it as
  a new copy. An old copy no longer hides that this computer needs to catch
  up from a snapshot.
- Copying one backup storage into another no longer puts records under a
  retired key over current ones.
- A silo whose main file was removed by a sync client opens from its spare
  copy instead of showing as unplugged.
- A photo the phone sent twice can no longer be imported with the wrong
  content.
- Release builds no longer run with the update signing key in reach. The
  installer is built first and signed afterwards, and in the release
  workflow only a separate job that builds nothing signs and publishes.
- The third-party notices cover every platform a bundle ships for, not only
  Windows.
- Texts that promised more than the app does are corrected. Replacing a
  recovery code or removing a key now says, before you confirm, that a
  never-delete copy keeps the old one. The emergency kit no longer names
  platforms or promises to say where your files are. Windows Hello and
  Touch ID are no longer asked to be touched or called a security key.
  Disconnect says it stops every copy. Sorting by size or date says
  smallest or newest first rather than A-Z. The 30-day note on deleting
  appears only when there is backup storage.
- A security key removed on one computer stays removed. Another computer
  that still had it could publish it again, and it came back everywhere.
- Leaving Favourites open no longer keeps the silo from locking itself.
- The new recovery code a key change makes is shown in its own window and
  stays until you say it is written down. It could be lost before.
- Adding files from Explorer into another open silo no longer leaves the
  window showing one silo while it works on the other.
- Enter or Delete on a dialog's button no longer opens or trashes the file
  selected behind it. Shift-click selects the files shown between the two
  clicks when the list is sorted. Ctrl+V adds copied files.
- A protected note's first line is no longer shown in lists or found by
  search before the key check.
- Key changes and the snapshot rebuild wait for a running backup pass
  instead of racing it.
- A file being decrypted when the silo locks is no longer opened afterwards.
- Removing a silo from the list keeps its working copy unless its files are
  deleted too, so changes not yet in a snapshot are not lost.
- Adding a second folder of a silo already in the list is refused instead of
  replacing it.
- Unlocking with a recovery code also tries the copy in backup storage, so a
  code replaced on another computer works here. A code made by a newer
  version says so instead of "doesn't match".
- A failed save of a password entry is undone on screen and keeps the
  editor open, and removed attachments are only deleted once the save
  succeeds.
- Storage timeouts are no longer reported as security key problems.
- A password with leading or trailing spaces survives a CSV export and
  import. A one-time code secret can be typed, and a mistyped one is refused.
- Turning autostart off in Task Manager shows as off here, and installing
  over an older version no longer turns it back on.
- A password copied to the clipboard is cleared even when another program
  was holding the clipboard at the moment the timer fired.
- Uninstalling removes decrypted copies of opened files.
- The daily update check now sends one request. Since 1.0.0 it usually
  sent two to four in the same second, because the check restarted every
  time the window redrew while the first request was still out. It was
  still once a day, and the requests carried nothing new.
- Switching to a silo that is already unlocked now reads its trash, the
  Explorer queue, the auto-import folders and the free disk space. Restored
  items show in Files at once, a failed delete puts the entry back, a
  damaged local copy can be rebuilt after a key unlock too, and a failed
  update install says so on the unlock screen.
- Imports keep custom fields, extra web addresses and one-time code secrets
  SilentSilo cannot use in the entry's notes, and say what went there and
  which passkeys were left out. A CSV export no longer puts a quote before
  phone numbers or @handles, and importing it again removes the quotes it
  added.

## [1.1.0] - Phones, and computers that agree

Windows only, like 1.0.0. The Android app and this release share a silo;
update every computer before adding a phone.

### Added

- An update the daily check found is marked on Settings in the sidebar, and
  Settings opens on Updates while it waits. Nothing is marked when automatic
  checks are turned off.
- Keys enrolled on another device, such as a phone, now appear here after a
  sync, and a key removed on one device stays removed on the others.
- Photos, contacts and shared files a phone sent while its silo was locked
  are added to the silo by the sync, under Phone backup.
- A login saved with a passkey from a phone can be edited and saved without
  a password.
- Copying a silo into another storage shows bytes as well as objects, so a
  single large file no longer looks like a stall, and Stop lands inside that
  file instead of after it. What already copied stays, and running it again
  carries on. A sync shows the bytes of the file it is uploading too.
- S3 uploads a file over 16 MiB in parts, which lifts the 5 GiB limit a
  single upload has. Parts a closed or crashed app left behind are cleared on the
  retry, and anything older than a day by the daily sweep, so none stay
  billed and out of sight.

### Changed

- Built on core 1.6.1. Nothing a silo holds changed in a way 1.0.0 cannot
  read, and 1.0.0 still opens a silo this version has used.
- Update every computer that uses the same silo. A computer still on 1.0.0
  can stop receiving other devices' changes after meeting some of them, and
  content only it holds can be lost when it cleans up storage. It picks
  everything up again once it updates.
- The working copy of a silo's index is now encrypted on disk while the
  silo is open and kept encrypted after it locks, so nothing readable is
  left behind by a crash, a forced quit or a power cut. Unlocking a silo you
  have opened on this computer before is faster than in 1.0.0; the first
  unlock after the update takes longer while the index is rebuilt.
- Locking a silo in which nothing changed no longer rewrites its snapshot.
- On macOS a key enrolled with the built-in authenticator is recorded as a
  Secure Enclave key rather than a FIDO2 credential, and every prompt names
  Touch ID where it used to name Windows Hello. Windows builds behave as
  before.
- Release groundwork for macOS: a platform config with the app and disk
  image targets, an ICNS and a menu-bar template icon rendered from the
  same SVG as the rest, and a release job that builds the universal app.
  Without a Developer ID in the repository secrets the job keeps its output
  as a workflow artifact and attaches nothing to the release.

- The domain crates moved to
  [silentsilo/core](https://github.com/silentsilo/core) and are now a pinned
  dependency rather than part of this workspace. Nothing a silo contains
  changed: the dependency graph was compared before and after, package for
  package and version for version, and the compatibility fixtures still
  rebuild a 1.0.0 silo from its storage. This repository keeps the
  application, its OS integration and the frontend.

### Fixed

- Signing out of Windows or shutting it down, including the restart after an
  update, left open silos' decrypted working copies on disk: the app is ended
  without being asked to quit. Every silo now locks on the way out.
- Decrypted copies a crash, a kill or a power cut left behind are deleted
  when the app starts, and after every lock, including those of silos that
  are never opened again. A file still open in another app when its silo
  locks is reported instead of silently kept.
- A file whose content is on no backup and not on this computer shows as
  Missing, and "Download everything" no longer offers it on every start.
- Installing an update on Windows ended the app without locking open
  silos, leaving their decrypted working copies on disk. Every silo locks
  before the installer starts, and a copied secret is cleared on quit.
- Creating a silo in a folder you picked no longer accepts a folder with
  other files in it, and removing a silo with its files deletes only what
  SilentSilo wrote there.
- The sync pass does not compact or sweep after a pass that could not read
  every record, checks the snapshot horizon against what it received rather
  than what it wrote, and keeps a newer recovery code made on another
  device instead of pushing its own back over it.
- A blank secret in the storage settings is kept only for the same server
  and account, and Test connection needs the silo unlocked.
- Removing or renaming a key, turning the recovery code off, removing a
  storage copy and adding a protected folder need the silo unlocked.
- Refreshing the file list after another device's changes no longer counts
  as use, so auto-lock still happens while other devices sync.
- A one-time code on an entry that asks for a key first is hidden until the
  entry is revealed.
- A password or TOTP secret starting with `=`, `+`, `-` or `@` was exported
  with a quote in front of it. Only the descriptive columns are guarded.
- The save dialog names the full destination and says when it is on
  another computer; the macOS Quick Action quotes the app's path.
- Turning the recovery code off did not stick: a second computer that still
  held the code put it back in storage on its next sync. Turning it off now
  leaves a marker every copy honours, and a code generated afterwards is
  dated after it, even on a computer whose clock is behind.
- A file moved on a computer that had not yet received another computer's
  edit or deletion of it could lose its content. Content a file still points
  at and a backup no longer holds is put back, from this computer or from
  another backup.
- Deleted content is removed from backup storage 30 days after nothing
  points at it rather than at once, so a computer that has not synced for a
  while can still reach it. Emptying the trash frees that space a month
  later.
- An edit made on one computer while another emptied the trash holding that
  file is kept, as a copy at the top of the silo, instead of being lost.
- Changes made here are written so that a computer still on 1.0.0 can apply
  them, where before some stopped it from syncing.
- The first unlock after an update that rebuilds the index took up to half a
  minute on a large silo; it is now several times faster.
- A silo locked straight after its key was changed failed to open again on
  that computer.
- Edge's autofill and its password manager are turned off inside the app's
  window, so nothing typed there, a revealed password included, can reach
  the browser's own unencrypted store. Crash reporting is off as well: a
  crash dump of the window would hold the decrypted password store.
- "Delete permanently" said nothing about the 30 days your storage keeps
  the content, or about an edit made on another computer at the same time
  coming back as a copy. Both dialogs say so now, and deleting a credential
  with attached files names the same 30 days.
- "Everything is backed up" counted records only, so a silo whose records
  had all been sent read as finished while a file was still queued or its
  upload had failed. The backup page now counts both, and a silo backed up
  to a folder, a WebDAV share or an SFTP server no longer reported zero
  records waiting whatever was queued.
- Saving several files, or a folder, replaced files of the same name in the
  destination without asking. It asks once, and leaves them alone unless
  told otherwise.
- Uninstalling left the Explorer menu entries behind, doing nothing on a
  right-click. They go now, with the queue files beside them, and the
  uninstaller says your silos are never deleted. README lists what stays.
- The window no longer stops responding while a long job runs. Every command
  the app answers now runs off the thread that draws the window, the places
  that held the silo's lock while talking to the keyring or the disk no
  longer do, and a target owed a long history is written down in one go
  rather than a database commit per record. The progress counters update
  once a frame instead of once per object, so Stop answers while a copy of
  hundreds of thousands of files is running.
- A storage copy could vanish from the list after it was added: Windows
  Credential Manager refuses more than 2560 bytes, one SFTP target with its
  private key passes that, and the app then read back the older, shorter
  list. The two copies of the list can no longer disagree.

### Security

- Joining a silo, with a key, with the recovery code, or when repairing a
  computer from storage, no longer trusts an "organisation" mark on the keys
  it finds there. Whoever can write to the storage could plant one and block
  key changes and recovery-code changes on that computer.
- The list of protected folders and the record of what was imported from
  them are encrypted. They named every mirrored file by its full path, in
  the clear. The files from 1.0.0 are converted and removed the first time
  the silo is opened, which is why that list now needs the silo unlocked.
- The recovery code's envelope in storage is checked before it is adopted,
  so storage can no longer bring back a code you turned off or replace it
  with one that opens nothing.
- An old copy of the silo's content key put back in storage is reported as
  that, instead of telling every computer to join again, which could not
  work.

## [1.0.0] - First public release

The first public version of SilentSilo: a local-first, end-to-end encrypted
vault for files and passwords.

- **Silos**: portable encrypted folders, several per install, each with its
  own keys, recovery code and backup storage. A silo can live on an external
  drive or inside a cloud-synced folder; nothing decrypted is ever written
  into it.
- **Unlocking**: FIDO2 hardware security keys or Windows Hello, both via
  `hmac-secret`, with a generated, written-down recovery code as the only
  fallback. Deliberately no passphrase option.
- **Files**: an explorer with grid and list views, drag & drop, global
  search over names, trash with restore, and Windows Explorer integration
  for adding and saving files.
- **Credentials**: an encrypted store for logins, cards, identities, SSH keys
  and notes, with TOTP (RFC 6238), a generator, a health view that finds
  reused and weak entries, and CSV import/export compatible with common
  password managers.
- **Runs in the notification area**: closing the window hides it instead of
  quitting, so the Explorer actions and scheduled backups reach the instance
  you already unlocked. On Windows it starts when you sign in, with no
  window and nothing unlocked; the Startup page in Settings turns that off.
- **Backup and multi-device sync**: optional, to storage the user controls.
  Any S3-compatible bucket, a WebDAV share, an SFTP server (with host-key
  pinning) or a plain folder. Devices converge through an append-only log of
  encrypted operations; the provider only ever sees ciphertext.
- **Sync that keeps its own house**: the log is compacted against a periodic
  snapshot, superseded content is swept from storage, joining reports a real
  proportion of the download, and two devices editing the same file offline
  produce a conflict copy instead of a silent loss.
- **Key rotation**: a revoked security key can be made to stop opening the
  silo without re-encrypting the content, because every blob carries its own
  key wrapped under a rotatable one.
- **Getting the data back without the app**: a verification pass rebuilds a
  silo from its storage and a recovery code and compares, a printable
  emergency kit carries what recovery needs, and `silentsilo-extract` reads
  a backup with no GUI involved. The steps it follows are written out in
  [`FORMATS.md`](https://github.com/silentsilo/core/blob/main/FORMATS.md) so a reader can reimplement them.
- **Cryptography**: AES-256-GCM throughout, envelope encryption per
  enrolled key, encrypted local index. Documented in
  [`docs/CRYPTO.md`](https://github.com/silentsilo/core/blob/main/docs/CRYPTO.md).
- **Formats**: everything written to disk or to storage carries a version,
  and a build meeting one it does not know says so instead of guessing. The
  local index is a rebuildable cache of the operation log, so a future
  release can change it without a migration. Listed in
  [`FORMATS.md`](https://github.com/silentsilo/core/blob/main/FORMATS.md), and checked on every build against a committed
  silo and its storage, rebuilt from a recovery code and compared. From here
  on those bytes are what a later release has to keep reading.

Known gaps are tracked honestly in [BACKLOG.md](BACKLOG.md). The installer is
code-signed, and an update is verified against the key built into the app
before it is installed.
