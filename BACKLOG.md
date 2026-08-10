# Backlog

What is knowingly left undone after the move to a serverless, S3-synced
desktop app. Nothing here blocks the app from working; each entry says what
breaks, and when.

## Storage backends

`ObjectStore` is in place with four implementations (S3-compatible, a plain
folder, WebDAV and SFTP) and a contract suite that runs the same assertions
against every one of them. Nothing here is outstanding.

One thing SFTP deliberately does not support: an SSH agent. Keys are pasted
in and kept with the silo's other secrets, so a silo works the same on a
machine with no agent running and after the key file moves. Agent forwarding
would be a convenience for one kind of user and a second code path to keep
correct for everyone.

## Keys

### Rotation cannot reach a target that refuses overwrites

Rotating the vault key re-seals the objects in storage under the new key. On
a target that refuses to overwrite, an append-only bucket or one under object
lock, the old objects keep their old envelopes, so a revoked key still opens
the copies someone already holds. The rotation itself succeeds and every
device stays consistent, which makes this a limit worth stating rather than a
failure to handle. Which targets behave this way is in
[`docs/STORAGE.md`](docs/STORAGE.md).

Nothing anywhere undoes the other half of it: what a person has already
copied stays copied.

## Product

### Packaging and signing

Installers are unsigned. On Windows that is a SmartScreen warning on every
download; on macOS it is a refusal to open at all.

## Housekeeping

### AGPL compliance check

Dependency licences have never been audited against AGPL-3.0.

## Done since this page was written

Kept here because the entries above used to include them, and a reader who
remembers the old list deserves to know where they went rather than assume
they were dropped:

- **Log compaction.** A periodic snapshot plus pruning of the records below
  its horizon, with `MANIFEST_VERSION` gating older clients out. Both the
  full and the compacted shape are covered by committed fixtures.
- **Blob garbage collection.** `sweep_orphan_blobs` deletes content no live
  operation references, run from a device that has replayed the whole log.
- **Key rotation.** A new vault key, re-wrapped envelopes and re-sealed
  records, with the content ciphertext left where it is. The remaining limit
  is the one above.
- **Conflict copies.** Two devices replacing the same file offline no longer
  lose the loser silently: it is kept beside the winner as
  `report (conflicted copy 2026-08-01).pdf`.
- **First-sync progress.** Joining reports a real proportion, because the
  listing arrives whole before the first object comes down.
- **Silo verification, the printable emergency kit, and the standalone
  extraction tool.** All three shipped.
