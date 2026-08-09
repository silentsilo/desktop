# Changelog

Notable changes are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). From this release
onward the version follows semver, and anything that could stop an existing
silo from opening needs a major version rather than a note.

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
  [`FORMATS.md`](FORMATS.md) so a reader can reimplement them.
- **Cryptography**: AES-256-GCM throughout, envelope encryption per
  enrolled key, encrypted local index. Documented in
  [`docs/CRYPTO.md`](docs/CRYPTO.md).
- **Formats**: everything written to disk or to storage carries a version,
  and a build meeting one it does not know says so instead of guessing. The
  local index is a rebuildable cache of the operation log, so a future
  release can change it without a migration. Listed in
  [`FORMATS.md`](FORMATS.md), and checked on every build against silos
  written by past releases.

Known gaps are tracked honestly in [BACKLOG.md](BACKLOG.md). The installers
are not code-signed yet, so Windows SmartScreen warns on download.
