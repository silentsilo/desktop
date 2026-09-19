# Changelog

Notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). From this release
onward the version follows semver, and anything that could stop an existing
silo from opening needs a major version rather than a note.

## [Unreleased]

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
  Filling needs a security key or Windows Hello on the silo.
- The extension's popup has an "Open SilentSilo" button when the silo is
  locked. It brings this window to the front, where you unlock as usual;
  then click the extension again. The extension cannot unlock anything.

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
