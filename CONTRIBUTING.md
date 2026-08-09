# Contributing to SilentSilo

Thanks for considering a contribution. This repository is the whole product:
a local-first, end-to-end encrypted vault (Tauri 2 + Rust + React). There is
no server component. Sync runs against storage the user controls.

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
npm test
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --all
```

This is what CI runs. If these pass locally, CI should too.

If your change touches anything in [`FORMATS.md`](FORMATS.md), read that page
first. `cargo test --all` includes the compatibility fixtures, which rebuild a
silo written by a past release and compare what comes out. A fixture whose
output changes means released data no longer reads the same way; the fix goes
in the code, never in the fixture.

The sync and storage integration tests need a real object store and skip
without one; see the README's *Integration tests* section for the one-line
MinIO setup.

## Making changes

- Keep PRs focused. A bug fix doesn't need an accompanying refactor.
- Read [`docs/CRYPTO.md`](docs/CRYPTO.md) before touching anything under
  `silentsilo-crypto` or `silentsilo-vault`. It is the source of truth for
  the key hierarchy and on-disk formats.
- Add or update tests for anything in `silentsilo-crypto`,
  `silentsilo-vault`, `silentsilo-vfs` or `silentsilo-sync`. These are the
  crates where a silent regression is most costly.
- If you change an on-disk or wire format (blob layout, `vault.db`
  encryption, envelope structure, operation-log records), bump the relevant
  version constant and update `docs/CRYPTO.md` in the same PR.
- Comments explain *why*, not *what*: an invariant, a workaround, a
  constraint the code cannot express. If the code needs a *what* comment,
  rewrite the code.

## Reporting security issues

Please don't open a public issue for a security vulnerability. See
[SECURITY.md](SECURITY.md).
