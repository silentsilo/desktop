## What does this change?

## Why?

## Checklist

- [ ] I have read [`CLA.md`](../CLA.md) and accept it for this contribution.
      Opening this pull request is acceptance either way; ticking it says you
      read it first.
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --all-targets -- -D warnings`
- [ ] `cargo test --all`
- [ ] `npm run lint`
- [ ] `npm run build`
- [ ] If this changes an on-disk/wire format (blob layout, vault.db
      encryption, envelope structure): version bumped and
      [`docs/CRYPTO.md`](../docs/CRYPTO.md) updated in this PR
- [ ] Tests added/updated for anything in `silentsilo-crypto`,
      `silentsilo-vault`, or `silentsilo-vfs`
