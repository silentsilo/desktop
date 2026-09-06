# SilentSilo

End-to-end encrypted vault for files and passwords, unlocked by a FIDO2
security key or Windows Hello, with a written-down recovery code as the
fallback. AGPL-3.0.

Everything runs locally: no account, no server. Backup is optional, to
storage you control: an S3-compatible bucket, a WebDAV share, an SFTP server
or a plain folder. Whatever holds it only ever sees ciphertext.

Every feature is in every copy. There is no paid tier, no licence key and
nothing held back.

> **No independent security audit has been done.** Nobody outside this
> project has been paid to attack it. The cryptography is specified in
> [`docs/CRYPTO.md`](https://github.com/silentsilo/core/blob/main/docs/CRYPTO.md),
> the formats in
> [`FORMATS.md`](https://github.com/silentsilo/core/blob/main/FORMATS.md),
> the threat model and its limits are published, and all of this is readable
> here. That makes the design reviewable; it is not the same as an audit, and
> a serious flaw could sit in code that looks right and passes its tests.
> Weigh that before trusting it with something whose disclosure would be
> severe for you. When an audit happens it will be published, findings
> included.

## What it looks like

| Unlocking, with a security key | The files in a silo |
|---|---|
| ![The unlock screen, waiting for a security key to be touched](docs/screenshots/unlock.png) | ![The file explorer in grid view](docs/screenshots/files.png) |

| Credentials | Backup, to storage you chose |
|---|---|
| ![The credentials view with a login selected](docs/screenshots/credentials.png) | ![The backup settings, connected to a folder](docs/screenshots/backup.png) |

## Stack

- Tauri 2 · Rust 1.97 · React 19 · Vite

## Where the code is

This repository holds the desktop application: the Tauri shell, the command
layer, the React frontend, and `silentsilo-shell`, which is the only crate
here that talks to the operating system.

Everything below the application lives in
[silentsilo/core](https://github.com/silentsilo/core): the cryptography, the
persisted formats, the operation log, sync, the storage backends, the
security-key backends and the standalone extraction tool. `Cargo.toml` pins a
tag from there, and the lockfile pins the commit. Mobile clients will pin the
same crates.

| Crate | Where | Role |
|-------|-------|------|
| `silentsilo` | here, `src-tauri/` | The app: commands, sessions, flows |
| `silentsilo-shell` | here, `crates/` | Explorer context menu, clipboard, autostart, tray plumbing |
| `silentsilo-crypto` | core | AES-GCM streaming, envelope encryption |
| `silentsilo-vault` | core | Silo provisioning; `vault.db` encrypted at rest (AES-256-GCM) |
| `silentsilo-vfs` | core | Folder/file tree, operation log |
| `silentsilo-fido` | core | FIDO2 security keys and Windows Hello |
| `silentsilo-s3` | core | S3-compatible object storage client |
| `silentsilo-store` | core | Backup storage: bucket, folder, WebDAV or SFTP |
| `silentsilo-sync` | core | Multi-device sync over whichever of those |

## Silos

One install can hold several (personal, family, work). Each is a folder you
choose the location of, holding its own encrypted index, blobs, security-key
envelopes and credentials. Nothing is shared between them but the app, which
keeps only an index of where they are; losing that index costs you the list,
not the data.

Because everything a silo needs is inside its folder, a silo can live on an
external drive, be copied to another machine, or be restored from a backup as
a unit. Nothing decrypted is ever written there: the working copy lives in a
machine-local directory and is wiped when the silo locks. A silo folder is
therefore safe to put anywhere, including a folder a cloud client is syncing.

That last case works as a backup for **one** computer. It cannot serve two:
the encrypted snapshot is a single file rewritten on every change, so two
machines editing it produce a conflict copy rather than a merge. Sharing a
silo between computers is what the operation log in a bucket is for.

## Unlocking

Three ways in, all of which end up handing the vault the same encryption key:

| Method | Strength | Survives the machine |
|--------|----------|----------------------|
| FIDO2 security key | `hmac-secret` on the key | Yes, carry it with you |
| Windows Hello | same, held in the TPM | No, sealed to that PC |
| Recovery code | 160 generated bits | Yes, it's on paper |

There is deliberately **no passphrase option**. The vault is only as strong as
its weakest envelope, and a memorable phrase is around forty bits. Allowing
one would quietly make it the real security of the whole design, particularly
since envelopes are published to the bucket so other devices can join.

## Silos a company provisions

A silo can be created as organisation-administered, which is a question asked
once, when the first key is enrolled, and off unless someone ticks it. The key
enrolled then stays the organisation's way in: the person using that computer
cannot retire it, cannot regenerate or disable the recovery code, and cannot
rotate the silo's encryption key without it. Each of those asks for one of the
organisation's keys and verifies it against the silo before going ahead.

It is meant for a company setting a silo up for an employee, so the archive
survives the employee leaving. Three things keep it from being a way to take
somebody's vault away from them: it can only be chosen while the silo is being
created, never added to one already in use; every device shows those keys with
an Organisation badge, so nothing about it is hidden from the person using it;
and it changes nothing about what the key can decrypt. An organisation key
unlocks exactly like any other.

The provisioning procedure, onboarding three ways and offboarding included,
is in [`docs/ORGANISATIONS.md`](docs/ORGANISATIONS.md).

## Optional backup and multi-device sync

Point the app at any S3-compatible bucket you control (AWS, Backblaze B2,
Cloudflare R2, Wasabi, MinIO, …) and it will keep itself in step with your
other devices. There is no server in the middle and nothing to sign up for:
the bucket is yours, and everything in it is ciphertext except a small
manifest naming a random vault id.

Sync is optional. A vault that never connects storage stays fully local and
fully usable.

Devices reconcile through an append-only log of operations. Every change is
one immutable object, so a push is always a create: no conditional writes,
no locking, and nothing that depends on a provider feature beyond plain
PUT/GET/LIST/DELETE. Devices that have been offline converge on the same
tree regardless of the order records arrive in.

To join a second device, pick *I already have a vault* on first run, point it
at the same bucket, and touch a security key already enrolled on the first.
Its tree is rebuilt by replaying the log. There is no snapshot to download,
and no step where anything is readable in transit.

## Dev

```bash
npm install
npm run tauri:dev
```

### Working on core at the same time

The core crates come from git at a pinned tag. To build the app against a
local checkout instead, create `.cargo/config.toml` (gitignored):

```toml
[patch."https://github.com/silentsilo/core"]
silentsilo-core = { path = "../silentsilo.core/crates/silentsilo-core" }
silentsilo-crypto = { path = "../silentsilo.core/crates/silentsilo-crypto" }
silentsilo-vault = { path = "../silentsilo.core/crates/silentsilo-vault" }
silentsilo-vfs = { path = "../silentsilo.core/crates/silentsilo-vfs" }
silentsilo-store = { path = "../silentsilo.core/crates/silentsilo-store" }
silentsilo-sync = { path = "../silentsilo.core/crates/silentsilo-sync" }
silentsilo-fido = { path = "../silentsilo.core/crates/silentsilo-fido" }
silentsilo-s3 = { path = "../silentsilo.core/crates/silentsilo-s3" }
```

That file rewrites `Cargo.lock`. Delete it and run `cargo check` to restore
the lockfile before committing; `node scripts/check-lockfile.mjs` says whether
the lockfile still points at core, and CI runs it.

The storage and sync integration tests live in core, along with the container
setup that feeds them.

```bash
./scripts/test-local.ps1
```

runs the whole CI sequence for this repository. `-RustOnly` skips the
frontend half.

### Platform notes

Security-key access uses a different backend per OS (both are real, working
implementations, neither a stub):

| OS | Backend | Notes |
|----|---------|-------|
| Windows | OS WebAuthn API (`webauthn.dll`) | No administrator rights required. |
| Linux / macOS | CTAP2 over USB HID (`ctap-hid-fido2`) | Linux typically needs `libudev-dev` and `libusb-1.0-0-dev` (or your distro's equivalents) installed to build the HID dependencies. |

`silentsilo-fido` lives in core and carries a `hardware` feature that pulls
in the HID stack. The app asks for it explicitly, so a build here always has
it; `--no-default-features` on the app turns off `custom-protocol` and
nothing else. To compile the crate without HID libraries, for a lint-only
environment, build it in core with `--no-default-features`, where it falls
back to a stub backend on which no security-key operation succeeds.

## Known gaps

See [BACKLOG.md](BACKLOG.md) for what is deliberately unfinished. No
independent security audit has been done, as the note at the top says. The
dependency licences have not been audited against AGPL-3.0 either, and there
is no licensing code. Log compaction, blob
garbage collection, key rotation, conflict copies, silo verification, the
printable emergency kit and the standalone extraction tool are all done.

## Docs

Crypto specification (public):
[`docs/CRYPTO.md`](https://github.com/silentsilo/core/blob/main/docs/CRYPTO.md),
in core

Setting up storage that survives a bad day, including append-only targets,
object lock, and the lifecycle rule that deletes your archive:
[`docs/STORAGE.md`](docs/STORAGE.md)

Persisted formats, their versions, and what an older build does when it meets
a newer one:
[`FORMATS.md`](https://github.com/silentsilo/core/blob/main/FORMATS.md), in
core
