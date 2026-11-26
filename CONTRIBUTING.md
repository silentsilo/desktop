# Contributing to SilentSilo

Thanks for considering a contribution. This repository is the desktop
application: a local-first, end-to-end encrypted vault (Tauri 2 + Rust +
React). There is no server component. Sync runs against storage the user
controls.

The engine underneath, the cryptography, the persisted formats, the operation
log, sync and the storage backends, lives in
[silentsilo/core](https://github.com/silentsilo/core) and is pinned here to a
tag. A change to any of that belongs in that repository; this one moves its
pin afterwards.

## Contributor License Agreement

Contributions are accepted under the terms of [CLA.md](CLA.md). Opening a
pull request constitutes acceptance; you keep the copyright to your work.
Please read it once before your first PR. It is short and written to be
readable.

## Dev setup

```bash
npm install
npm run tauri:dev
```

Opening the app in a plain browser with `npm run dev` and `?mock` in the URL
stubs the Rust side, which is the fastest way to work on the pre-unlock
screens (picker, enrolment, unlock, recovery). The mock is compiled out of
release builds.


On Windows, turn on long paths for git first (`git config --global
core.longpaths true`): cargo's checkout of core passes the 260-character limit
without it.

### Working on core at the same time

The core crates come from git at a pinned tag. To build the app against a
local checkout instead, create `.cargo/config.toml` (gitignored):

```toml
[patch."https://github.com/silentsilo/core"]
silentsilo-app = { path = "../silentsilo.core/crates/silentsilo-app" }
silentsilo-audit = { path = "../silentsilo.core/crates/silentsilo-audit" }
silentsilo-cloud = { path = "../silentsilo.core/crates/silentsilo-cloud" }
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

Use it rather than a bare `cargo test`. Several tests create a silo's work
directory, which is derived from the silo's path and lives outside it, so an
unguarded run leaves a uuid-named directory in
`%LOCALAPPDATA%\SilentSilo\work\open` for every run, in the place a real
silo's scratch goes. The script points `SILENTSILO_TEST_WORK_BASE` at
`target\test-work` and clears it afterwards; CI does the same with the
runner's temp. Set the variable yourself if you run cargo directly:

```powershell
$env:SILENTSILO_TEST_WORK_BASE = "$PWD\target\test-work"
```

### Platform notes

Security-key access uses a different backend per OS (both are real, working
implementations, neither a stub):

| OS | Backend | Notes |
|----|---------|-------|
| Windows | OS WebAuthn API (`webauthn.dll`) | No administrator rights required. |
| Linux / macOS | CTAP2 over USB HID, core's own `ctap2` code through `hidapi` | A key with a PIN is asked for it in the app, as Windows does. Building on Linux needs `libudev-dev` (or your distro's equivalent). Using a key needs the udev rule for FIDO keys, which the `libfido2` package installs. |

`silentsilo-fido` lives in core and carries a `hardware` feature that pulls
in the HID stack. The app asks for it explicitly, so a build here always has
it; `--no-default-features` on the app turns off `custom-protocol` and
nothing else. To compile the crate without HID libraries, for a lint-only
environment, build it in core with `--no-default-features`, where it falls
back to a stub backend on which no security-key operation succeeds.

## Before opening a PR

```bash
npm run typecheck
npm run lint
npm test
npm run build
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --all --locked
cargo check --all --locked
node scripts/check-lockfile.mjs
```

`./scripts/test-local.ps1` runs all of it in order. This is what CI runs. If
these pass locally, CI should too.

`--locked` is not decoration. Working on core at the same time means a
`[patch]` in `.cargo/config.toml`, which rewrites `Cargo.lock` and points the
build at a checkout on your machine. Committed, that lockfile builds the
release against a sibling directory. `--locked` and the lockfile check are
what catch it.

If your change needs something from core, say so in the PR rather than
vendoring it here. The compatibility fixtures and the storage integration
tests live there too.

## Making changes

- Keep PRs focused. A bug fix doesn't need an accompanying refactor.
- Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) before changing the
  session lifecycle or the order a sync pass runs in. Core's own map covers
  everything below the application.
- The commands are the contract with the frontend, and so are their
  parameter names, the event names and payload shapes, and the error strings
  `src/lib/errors.ts` matches on. All four are matched by string and fail
  silently when renamed. Change the frontend in the same commit.
- Nothing about a persisted format can change from here. That is core's
  jurisdiction, and it has its own rules.
- Comments explain *why*, not *what*: an invariant, a workaround, a
  constraint the code cannot express. If the code needs a *what* comment,
  rewrite the code.

## Reporting security issues

Please don't open a public issue for a security vulnerability. See
[SECURITY.md](SECURITY.md).
