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

See the [Platform notes](README.md#platform-notes) in the README for the
security-key backend differences (Windows vs. Linux/macOS) and Linux build
dependencies.

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
- The 107 commands are the contract with the frontend, and so are their
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
