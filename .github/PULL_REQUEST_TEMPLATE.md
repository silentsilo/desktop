## What does this change?

## Why?

## Checklist

- [ ] I have read [`CLA.md`](../CLA.md) and accept it for this contribution.
      Opening this pull request is acceptance either way; ticking it says you
      read it first.
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets --locked -- -D warnings`
- [ ] `cargo test --all --locked`
- [ ] `cargo check --all --locked`
- [ ] `node scripts/check-lockfile.mjs`
- [ ] `npm run lint`
- [ ] `npm run build`
- [ ] `Cargo.lock` still points at silentsilo/core, not at a local checkout
- [ ] If this renames a command, a parameter, an event or an error string,
      the frontend changed in the same commit
