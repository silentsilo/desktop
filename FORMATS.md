# Persisted formats

Everything SilentSilo writes that outlives a run, what version it is at, and
what an older build does when it meets a newer one.

A vault that will not open is unrecoverable by the person who owns it. That
makes every change on this page higher risk than the code around it, so each
one answers the same question before it lands: **what does a client on the
previous release do with the new bytes?** Only two answers are acceptable, and
they have to be true by test, not by argument.

1. It ignores the new thing safely.
2. It refuses explicitly and says to update.

Anything else needs a version discriminator first.

## In the bucket

| What | Where | Version | An older build meeting a newer one |
|---|---|---|---|
| Manifest | `vault.json` | `MANIFEST_VERSION = 1`, `silentsilo-sync` | Refuses to open the silo and says to update |
| Snapshot | `snapshots/….snap` | `SNAPSHOT_VERSION = 1`, `silentsilo-vfs/snapshot.rs`, sealed | Refuses, naming the version |
| Operation record | `ops/…` | `skippable` flag per record, plus `seq` and `prev`, `silentsilo-vfs/oplog.rs` | Skips a record marked ignorable, refuses on anything else. A record without `seq` is refused outright. `ReplaceFileContent.replaces` is optional and absent when unset, so a record without it decodes and is applied the old way, without conflict detection |
| Sealed payload | wraps every op record | `SEAL_VERSION = 1`, `silentsilo-crypto/sealed.rs` | Refuses, naming the version |
| Blob | `blobs/….sslo` | `SSLO_VERSION = 1`, `silentsilo-crypto/blob.rs` | Refuses, naming the version |
| Recovery envelope | `recovery.env` | `RECOVERY_ENVELOPE_VERSION = 1`, plus stored KDF parameters | Refuses on a newer structure; a parameter change needs no bump |
| Key envelopes | `keys/….env` | **No file version.** Plain JSON of one enrolled key, carrying a `kind` field that says how its `wrapped_dek` is unwrapped | Ignores an envelope whose `kind` it does not know, and refuses by name when none is left it can use |
| Content KEK | `keys/content.kek` | Sealed payload under the vault DEK | Refuses, naming the version |

## Reading a backup from scratch

The order below is the whole of it, and it is written out because a format
nobody can read without our source is not a format. `silentsilo-extract`
follows exactly these steps; a reader written from this page alone should
arrive at the same files.

Everything is in one bucket or folder. Nothing else is needed but the
recovery code.

1. **`vault.json`** gives `vault_id` and `MANIFEST_VERSION`. Refuse a version
   above the one you understand rather than guessing at the rest.

2. **`recovery.env`** holds the vault DEK wrapped under the recovery code.
   Derive the wrapping key with Argon2id over the normalised code and the
   salt stored beside it, using the parameters in the envelope rather than
   assumed ones, then open the sealed payload. Out comes the 32-byte DEK.
   Normalising means uppercasing, dropping separators, and mapping the
   letter and digit pairs the alphabet excludes.

3. **`keys/content.kek`** is a sealed payload under the DEK. Inside is the
   32-byte content KEK. Every file's key is wrapped under this, not under the
   DEK, which is what lets the vault key be rotated without touching content.

4. **`snapshots/`**, if it is not empty. Take the highest-numbered object,
   which is also the last in a plain listing because the horizon is
   zero-padded. It is a sealed payload under the DEK holding the whole tree
   at that point. Start from it: after a compaction the records below its
   horizon are gone, and replaying only what is left rebuilds the tail of the
   history rather than the silo.

5. **`ops/`**, in listing order, which is already the order to apply them: the
   key begins with the zero-padded Lamport counter. Each object is a sealed
   payload under the DEK holding one JSON operation record. Apply them onto
   the snapshot, or onto an empty tree if there was none. A record whose
   operation you do not recognise is skippable if it says so and fatal if it
   does not.

6. **The tree** is then folders, files, and for each file its `blob_id` and
   its `blob_key`. Unwrap `blob_key` with the content KEK from step 3 to get
   that file's own 32-byte key.

7. **`blobs/<blob_id>.sslo`** is the content. The 82-byte header gives the
   version, the chunk size, the file and blob ids, the BLAKE3 hash of the
   plaintext and the nonce prefix. After it, chunks of `chunk_size`
   ciphertext each followed by a 16-byte tag. Decrypt each with AES-256-GCM
   under the file's key, nonce being the 8-byte prefix followed by the
   big-endian chunk index, with the blob id as additional authenticated data.
   Check the plaintext against the hash in the header: the header is not
   authenticated, so that comparison is what catches content swapped for
   other valid content.

Two things worth stating because they are easy to get wrong. Records are
never rewritten, so a record's fingerprint covers a body that does not
change, and the `prev` chain per device holds. And a blob can be in storage
before the record naming it, so content nothing refers to is normal rather
than a sign of damage.

## In the silo folder

| What | Where | Version | Notes |
|---|---|---|---|
| Wrapped DEK | `master.dek.enc` | Sealed payload envelope | Same format as an op record, one place to change |
| Credentials | `credentials.json` | `SILO_FILE_VERSION = 1`, `silentsilo-vault/format.rs` | Refuses rather than reading as "no silo here" |
| Storage settings | `s3.json` (or the keyring) | `SILO_FILE_VERSION` | |
| Enrolled keys | `keys/fido.json` | `SILO_FILE_VERSION`, plus a `kind` per key | |
| Recovery envelope | `keys/recovery.json` | Its own, as above | Its version means the shape of one wrapped key, not the file |
| Index | `vault.db.enc` | `SCHEMA_VERSION = 1`, `silentsilo-vfs/schema.rs` | Not a format: see below |
| Base snapshot | `vault_base` table in `vault.db` | `SNAPSHOT_VERSION = 1`, `silentsilo-vfs/snapshot.rs` | Refuses, naming the version |

## The kind of a key

Every enrolled key carries a `kind`, in `keys/fido.json` and in the envelope
published to `keys/….env`. Today there is one value, `fido2`: a FIDO2
credential whose `hmac-secret` output derives the key that unwraps
`wrapped_dek`. An envelope written before the field existed reads as `fido2`,
which is what it is.

The field is not for this build. It is for the one that meets a key it has
never heard of, and that is not a hypothetical: a macOS build would enrol
Touch ID keys through the Secure Enclave, and a Linux one may use a TPM.
Neither derives anything a FIDO2 backend can produce, and every device on a
silo reads every other device's envelopes.

So the rule for anything reading these:

- **Publishing, listing and revoking go over every enrolled key.** A key this
  build cannot use is still a key on the silo. Retiring another platform's
  key because it looks unusable is how a household ends up with one device
  locked out.
- **Unlocking goes over the keys of a kind this build knows.** That is
  `StoredFidoKeys::usable`, and it is what the authenticator allow-list, the
  "do you have a spare key" nudge and the last-key removal guard are all
  counted from. A credential id only has to be hex because a FIDO2 one is:
  built from every key instead, a single id in another platform's shape would
  fail the whole unlock on a machine whose own key is plugged in.
- **When nothing is left that this build can use, refuse by name.** Joining a
  silo whose only keys are of another kind says so, rather than asking for a
  security key that cannot exist.

Adding the field is only possible before the first public release. The
clients that have to skip an unknown kind are the ones already installed, and
they cannot be taught after the fact.

`kind` is also the version, and there is deliberately no second field beside
it. A key envelope is the one thing in the bucket with no file version, and
the fix for that is not to add one now: a version added today is a number
shipped clients do not check, so it would buy nothing that `kind` does not
already buy. If the shape of a `fido2` envelope ever has to change
incompatibly, ship it as a new kind. Every client from the first release
onward skips what it does not recognise and says so when nothing is left,
which is the behaviour a version field would have been for.

## In app data

| What | Version |
|---|---|
| Silo list | `SILO_FILE_VERSION` |

## The index is not a format

Every table in `vault.db` except `vault_meta` and `oplog` is a cache of the
operation log. A version mismatch is resolved by dropping the derived tables
and replaying, never by migrating in place, which is what makes changing the
schema free.

That only holds while the log is complete on disk, and "complete" now means
one of two things. Either `oplog` holds the whole payload of every record this
device has seen, which is the state of every silo that has not been compacted,
or it holds every record above a horizon and the `vault_base` table holds the
state at it. A rebuild restores that snapshot first and replays what is left
on top.

The snapshot is therefore the one row in `vault.db` that a replay cannot
produce, since it stands in for the records a replay no longer has. It is a
real format, versioned, and it is why `snapshot::prune_to` never drops a
record the bucket has not confirmed: until a record is pushed, the log row is
its only copy.

Two consequences worth stating:

- Adding a `VaultOp` variant must bump `SCHEMA_VERSION`. The rebuild is what
  applies records an earlier build stored but could not act on.
- Adding a `VaultOp` variant must also decide `VaultOp::is_skippable`. The
  match is exhaustive, so it will not compile until someone does.

## The fixtures

`crates/silentsilo-fixture/fixtures/<version>/` holds a silo and its storage
as written by that release, plus the exact contents they should produce.
`cargo test -p silentsilo-fixture` rebuilds each one from storage using only a
recovery code and compares.

Each era has two fixtures, not one. `v<version>` holds the full operation
log; `v<version>-compacted` holds the same silo after compaction, a snapshot
in the store and the records below its horizon pruned. They must produce the
same tree, and they prove different halves of recovery: the plain one that a
whole log still replays, the compacted one that a snapshot still restores and
the tail still lands on top of it. Without the second, the snapshot format
has no golden bytes and a break in it passes every test.

A change that alters a fixture's output is a break. The fixture is the
released format; updating it to match new behaviour only hides the problem
from the people who will hit it.

The one exception, and it expires with this release: while nothing had
shipped, the formats iterated freely and the fixture was regenerated to match,
because no silo written by an older iteration existed outside this repository.
From 1.0.0 onward that reasoning is gone and the rule above is the whole
rule.

## Per-blob content keys

Content is not encrypted under the vault DEK. Each blob gets a key of its
own, and that key travels wrapped under the DEK inside the operation record
that names the blob, and inside the snapshot once compaction has replaced
those records.

The reason is rotation. Rotating the vault key has to make a revoked security
key stop reading the silo, and with content under the DEK that would mean
re-encrypting every byte: hours or days for a large archive, and impossible on
a target that refuses deletes. With a key per blob the ciphertext never moves.
Rotation re-wraps a few hundred bytes per file and re-seals the records, and
someone holding the old DEK can no longer open the current records, so they
cannot reach the content key, so the unchanged ciphertext is closed to them.

Where each key lives, and where it deliberately does not:

- `encrypt_file` and `decrypt_blob` take a `ContentKey`, never the
  `MasterDek`. The key is not in the `.sslo` file: a key inside the object it
  protects would have to be rewritten to rotate, which is the cost being
  avoided.
- `AddFile` and `ReplaceFileContent` carry `blob_key`, the wrapping.
- `files` carries a `blob_key` column, rebuilt from the log like the rest.
- `FileRow` in a snapshot carries `blob_key`. A snapshot stands in for the
  records it replaced, so without this the keys would be lost at the first
  compaction.
- A password attachment's key lives in the sealed entry, its only reference.

### The content KEK

Content keys are wrapped under a key of their own, `content.kek.enc` in the
silo folder and `keys/content.kek` in storage, itself sealed under the vault
DEK. Password entries are sealed under it too.

The reason is the log's hash chain. `OpRecord::fingerprint()` is a hash of the
whole record, so anything inside a record can never be rewritten without
changing its fingerprint, breaking the `prev` chain, and leaving this device
disagreeing with every other about the bytes of the same operation. Both the
wrapped content key and a sealed password entry live inside records.

So the values in records never change, and rotation touches one key instead.
It re-wraps the KEK under the new DEK and re-seals the objects in storage,
whose bodies stay byte for byte identical. Someone holding the old DEK can no
longer open the current records, so the wrapping inside is out of reach even
though it has not moved.

Every device shares one KEK, fetched from storage when joining. A device that
minted its own would write files none of the others could open, so a join with
no KEK in storage is refused rather than made to work.

A silo written before this does not open. That was acceptable exactly once,
for the reason above, and the rule at the top of this page is the whole rule
from here on.

Add a fixture when a release changes anything on this page:

```
cargo run -p silentsilo-fixture -- create crates/silentsilo-fixture/fixtures/v<version>
```

One per format era rather than one per tag. Ten releases that change nothing
here share one fixture, named for the release where that format first shipped;
ten copies of the same bytes would prove nothing and cost a review every time.

Old fixtures are never removed.

`v1.0.0` opens the era this page describes: per-blob content keys, the
content KEK, and password entries sealed under it. It is the first era with a
public release behind it, so it is also the first that can never be
regenerated.
