# SilentSilo Cryptography Specification (v1)

Public reference for auditors and contributors. Describes what the desktop
app does. Everything below happens on the user's machine: there is no server
component and no account. The only network calls are the optional backup and
sync against storage the user configures (an S3-compatible bucket, WebDAV,
SFTP or a folder), which carry the same ciphertext described here. The
storage provider sees opaque blobs, encrypted operation records, per-key
envelopes whose wrapped DEK is ciphertext but whose surrounding fields are
not, and one small manifest naming a random vault id. "What the storage
provider learns" below is the full list, and it is longer than the sentence
above suggests.

## Threat model (summary)

| Asset | Location | At rest |
|-------|----------|---------|
| File names, folder tree | `vault.db` (plaintext working copy, only while unlocked) + `vault.db.enc` | AES-256-GCM |
| File content | `.sslo` blobs under the vault directory | AES-256-GCM, chunked |
| Master DEK | `master.dek.enc`, `keys/fido.json`, `keys/recovery.json` | Wrapped separately under each key that can produce it |
| Security-key secret | Hardware token (any FIDO2 key with hmac-secret) | Never leaves the device |

An attacker with the vault directory but no enrolled security key has
ciphertext only. While the vault is unlocked, the plaintext `vault.db` working
copy exists on disk and is deleted on lock or exit. It lives outside the vault
directory, under the machine's local application data, and so does the
fallback file holding the device secret when the OS keyring will not take it.
Neither travels with the folder.

Two limits of that sentence, stated here rather than left to be discovered:

**A removable key present at the machine is enough on its own.** Ceremonies
ask for user verification as `discouraged`, so a key already plugged in
unlocks the vault with one touch: no PIN, no biometric. The credential id and
the salt (`silentsilo-dek-v1:{vault_id}`) are both readable from
`keys/fido.json` and from storage, so a program running as the user can talk
CTAP2 to the key directly and derive the wrap key on the next touch the user
makes for any reason. Someone who holds both the folder and the key, even for
a few seconds, opens the vault. Requiring verification is a setting worth
having and does not exist yet.

**Removing a key does not change the key.** Removing a security key or a
recovery code deletes its envelope. The DEK itself is untouched, so anyone who
already derived a wrap key, or who kept a copy of the vault directory or of
the bucket, can still decrypt everything that existed then and everything
written afterwards. On its own, removal is effective against someone who has
to come back for the data, not against someone who already took it.

Rotating the vault key is what closes that, and it is implemented. It gives
every object in storage a new envelope and re-wraps the key under only the
credentials carried through, so a removed key stops opening anything written
before or after. Two things it still cannot do: reach a target that refuses
overwrites, where the old objects keep their old envelopes; and take back what
someone has already copied. Nothing anywhere undoes that second one.

## Key hierarchy

```
Security key (hmac-secret) ─┐
Device secret ─────────┼─► wrap_key [32] ──► sealed envelope wraps master.dek.enc
Recovery code ─────────┘

Master DEK [32]  ──► sealed envelope: vault.db.enc, operation records,
                  │    password entries
                  └► wraps the content KEK

Content KEK [32] ──► wraps one content key per blob, carried inside the
                     operation record that names the blob

Content key [32] ──► AES-256-GCM chunked blob encryption (.sslo)
```

Content is not encrypted under the Master DEK directly. Each blob has a key
of its own, wrapped under the content KEK, which is itself stored wrapped
under the DEK (`content.kek.enc`). That indirection is what makes rotating
the vault key affordable: the ciphertext never changes, the KEK is re-wrapped
and the records re-sealed, and the old key stops reaching any content key.
See [FORMATS.md](../FORMATS.md), section "Per-blob content keys".

Three keys can produce the Master DEK, each wrapping the same value
independently: the key derived from an enrolled authenticator, the one derived
from this machine's device secret, and the one derived from the written-down
recovery code. Any of them opens the vault on its own, and none can be derived
from another.

### Device provisioning secret

Until security-key enrollment completes, the **device secret** (32 bytes of OS randomness, generated locally at vault creation) derives a local wrap key via **Argon2id**:

| Parameter | Value |
|-----------|-------|
| Algorithm | Argon2id v0x13 |
| Memory (m) | 19 456 KiB |
| Iterations (t) | 2 |
| Parallelism (p) | 1 |
| Output | 32 bytes |

Salt: 16-byte random per vault (`vault.salt` file). This derived key wraps the Master DEK (it no longer keys `vault.db` directly; see below).

After security-key enrollment, `master.dek.enc` is re-wrapped to the FIDO-derived key; the device secret no longer unlocks the DEK.

## Security-key unlock (FIDO2, CTAP2 hmac-secret)

Vendor does not matter. Any FIDO2 authenticator that implements the
`hmac-secret` extension can be enrolled: YubiKey 5, the Yubico Security Key
series, Nitrokey 3, SoloKeys, Token2 and Feitian keys among others, plus
Windows Hello as a platform authenticator (which needs the PRF support
added to Windows 11 25H2 in February 2026). On Windows the app goes through
`webauthn.dll`, the same service the browsers use, so transports and PIN
prompts are the OS's business. U2F-only keys do not work: they have no
`hmac-secret`. Phones reached over the QR-code flow work when their
passkey provider offers the extension over hybrid transport, which recent
Android phones with Google Password Manager do; a phone that does not is
refused at enrollment with a message saying why. The gate is the
capability, checked in the attestation, never the device type. Note the
trust model: a phone passkey syncs with the account behind it rather than
staying pinned to the hardware, and every unlock runs the Bluetooth
ceremony with the phone present.

| Field | Value |
|-------|-------|
| RP ID | `silentsilo.com` |
| Extension (make credential) | `hmac-secret: true` |
| Extension (get assertion) | `hmac-secret` with salt string |
| Salt string | `silentsilo-dek-v1:{vault_uuid}` |

Flow:

1. **Enrollment**: `makeCredential` with hmac-secret extension; store `credential_id` + COSE public key locally (`keys/fido.json`).
2. **Unlock**: `getAssertion` with hmac-secret salt; the key returns 32-byte HMAC output.
3. **wrap_key**: `BLAKE3(hmac_output)` → 32-byte AES-256 key.

For Windows Hello, enrollment is a single ceremony: the salt goes in with
`makeCredential` (`pPRFGlobalEval`, raw-salt flag) and the HMAC output comes
back attached to the attestation (`pHmacSecret`, structure version 7), so the
wrap key exists before any assertion runs. Safe there because Hello verifies
the user on every ceremony, so both of CTAP's per-credential secrets collapse
into one. A removable key still derives through step 2 at enrollment too: its
unlock asks for verification `discouraged`, and a key that verified at
creation would otherwise hand back the other secret, wrapping the DEK under
a value no unlock ever reproduces.

Slots are assigned in enrollment order (`0` for the first key), but carry no
privilege difference: every enrolled key wraps the same DEK and can unlock the
vault on its own. Enrolling a second key is redundancy against losing the first.

## Enrolled key records (`keys/fido.json`)

One record per enrolled credential, stored locally alongside the vault:

| Field | Content |
|-------|---------|
| `credential_id` | FIDO credential id (hex) |
| `wrapped_dek` | Sealed envelope (below) holding the Master DEK under that key's wrap key |
| `key_slot` | Enrollment order, `0` for the first key |
| `public_key` | COSE/DER public key |
| `policy` | Empty, or `org` for a key an organisation administers |

Each record independently wraps the same Master DEK, which is what lets any
enrolled key open the vault on its own. Removing a key deletes its record, so
that key can no longer unwrap the DEK.

`policy` is the one field here that is not about cryptography, and it is worth
being precise about what it does and does not do. It does not restrict what a
key can decrypt: an `org` key wraps the same DEK as every other and opens the
vault identically. What it constrains is administration, enforced by the app
rather than by the mathematics. On a silo whose first key was enrolled as
organisation-administered, retiring such a key, regenerating or disabling the
recovery code, or rotating the vault key (which is both of those in one
operation) requires a fresh assertion against one of those keys, verified by
unwrapping its envelope.

The honest limit follows from that: the guarantee is only as strong as the copy
of the data an organisation controls. Someone who edits `keys/fido.json` by hand
or runs a modified build can clear the field locally, because it is their disk.
What they cannot do is remove the key from a backup target the company owns and
has marked append-only, which is where the escrow actually lives. Treat the
field as a rule the honest client enforces and the company's own storage backs
up, not as a cryptographic lock. See [FORMATS.md](../FORMATS.md) for the exact
rules.

The file itself is written inside the settings envelope described in
[FORMATS.md](../FORMATS.md), so a build meeting a file it cannot read reports a
version rather than reporting no enrolled keys.

## Sealed payload envelope

One shape for every small payload encrypted under a 32-byte key: the wrapped
Master DEK, the `vault.db` snapshot, each operation record leaving the machine,
and each password entry.

```
[ magic "SSEA" (4) ][ version (1) ][ nonce (12) ][ ciphertext ][ tag (16) ]
```

| Field | Value |
|-------|-------|
| Cipher | AES-256-GCM |
| Key | Master DEK, or a 32-byte wrap key |
| Nonce | 12 bytes from the OS CSPRNG, fresh per payload |
| AAD | The 5 header bytes (magic and version) |
| Version | `1` |

The header is authenticated, so a payload cannot be relabelled as an older
version to make a reader interpret it under weaker rules. A version this build
does not know is refused by name rather than guessed at.

A fixed nonce was used here in a pre-release version and is no longer accepted.
It offered no protection against a later re-wrap reusing a (key, nonce) pair,
which for AES-GCM leaks the XOR of both plaintexts and the authentication
subkey. Payloads in that form are rejected on read.

### Local DEK wrap

| Field | Value |
|-------|-------|
| Plaintext | 32-byte Master DEK |
| Key | 32-byte `wrap_key` (device secret, FIDO-derived, or recovery code) |
| Format | Sealed envelope, above |
| Output file | `{vault_root}/master.dek.enc` |

## Recovery code (`keys/recovery.json`, `recovery.env` in storage)

The only alternative to an enrolled authenticator, and deliberately the only
one. There is no passphrase option anywhere in the design: the vault is as
strong as its weakest envelope, and a memorable phrase is roughly forty bits.

| Field | Value |
|-------|-------|
| Code | 160 bits from the OS CSPRNG, shown as 32 Crockford Base32 characters |
| KDF | Argon2id v0x13, m = 65 536 KiB, t = 3, p = 1, 32-byte output |
| Salt | 16 bytes, random per envelope, stored with it |
| Wrapped DEK | Sealed envelope, above |
| Envelope version | `1` |

The KDF parameters are **stored in the envelope** rather than assumed by the
code that reads it. Retuning them, which a security audit is a likely reason to
do, must not invalidate codes already written on paper.

Heavier than the device secret's parameters because this envelope is published
to storage. Not because the code needs it: 160 bits is beyond brute force
whatever the provider does with the envelope.

The generated code exists in readable form exactly once, when it is created.
Nothing derived from it is kept except the envelope, so it cannot be shown
again, and replacing it invalidates the previous one.

## Blob format (`.sslo` v1)

On-disk layout:

```
[ header 82 bytes ]
[ chunk 0: ciphertext | tag(16) ]
[ chunk 1: ... ]
...
```

Chunks carry no per-chunk nonce material on disk: the nonce is derived from
the header's random prefix plus each chunk's position in the stream, so both
encrypt and decrypt reconstruct it from a simple running counter.

### Header (big-endian)

| Offset | Size | Field |
|--------|------|-------|
| 0 | 4 | Magic `SSLO` |
| 4 | 2 | Version `1` |
| 6 | 4 | Chunk size (default 4 194 304) |
| 10 | 16 | `file_id` UUID |
| 26 | 16 | `blob_id` UUID |
| 42 | 32 | BLAKE3 content hash (plaintext) |
| 74 | 8 | Random nonce prefix (unique per file) |

The version is compared exactly, and anything else is refused by name. Two
earlier pre-release layouts existed and were retired before anything shipped;
the numbering restarted at 1 for release, so no data in the world carries
another number. The check stays exact rather than a floor because both retired
layouts were weaker in ways an attacker could ask for by relabelling a header.

One had no nonce prefix, so the chunk nonce was the chunk index alone. Every
file then shared one key, which made that nonce collide across every file at
the same chunk position, and reusing an AES-GCM (key, nonce) pair breaks both
confidentiality and authentication.

The other bound nothing into the AAD. A party with write access to storage
could move a chunk from one blob to another and it would still decrypt and
authenticate, because nothing tied the ciphertext to the file it claimed to
be.

### Chunk encryption

| Field | Value |
|-------|-------|
| Cipher | AES-256-GCM |
| Key | The blob's own 32-byte content key (see the key hierarchy above) |
| Nonce | 12 bytes: random 8-byte per-file prefix (from header) + `chunk_index` as u32 BE |
| AAD | `blob_id` (16 bytes), so a chunk cannot be substituted from another blob |
| Plaintext | Up to 4 MiB per chunk |

Content hash in header is BLAKE3 of concatenated plaintext chunks, computed
during encryption and written back into the header. Both decryption and
verification recompute it and refuse a mismatch. Each chunk authenticates on
its own, so a file cut at a chunk boundary would otherwise decrypt cleanly
and come out silently shorter; the hash is what catches that. The header is
not authenticated, but the hash covers plaintext an attacker never sees, so a
matching value cannot be written in for tampered content.

## What deleting does not remove

Stated here rather than left to be discovered, because the app uses the word
"permanently" and this is where that word stops being complete.

Deleting a file and purging it removes the blob the index points at, which is
its current content. Replacing a file's content writes a **new** blob and
leaves the previous one where it is, and nothing collects those. They stay in
the silo, and in storage if one is configured, readable with the Master DEK,
for as long as the vault exists.

So a file edited three times and then deleted leaves two earlier versions
behind. Someone who replaces a document to remove something from it, then
deletes the file, has not removed what they thought they had.

The password store behaves the same way for a different reason: replacing a
password writes a new operation record, and the previous value stays in the
log. Records are never pruned today.

| Action | Removed | Left behind |
|--------|---------|-------------|
| Purge a file | Its current blob | Blobs superseded by earlier content replacements |
| Change a password | Nothing | The previous value, in the operation log |
| Revoke a security key | Its envelope | The DEK it wrapped, unchanged |
| Disable the recovery code | Its envelope, locally and in storage | The DEK it wrapped, unchanged |

The last two rows are the ones that read as stronger than they are. Deleting an
envelope closes a door; it does not change the lock. Whoever derived the wrap
key while the key was enrolled, or kept a copy of the vault directory or the
bucket from before, still holds a working path to the DEK and therefore to
everything in the silo, past and future. See the threat model above.

Revocation also depends on reaching storage. Removing a key marks it locally
straight away and keeps retrying the deletion of its published envelope on
every sync pass until storage confirms it; the app says so when the first
attempt does not get through. Until it does, that key still opens the silo from
another machine. And a second device that has not yet learned of the removal
republishes the envelope on its own next pass, so revocation converges on one
device rather than all of them until a shared revocation record exists.

On storage that keeps what it is asked to delete, revocation does not happen
at all. A versioned S3 bucket answers a DELETE by writing a delete marker and
keeping the previous version, readable by anyone holding
`s3:GetObjectVersion`; object lock refuses the DELETE outright until the
retention expires. So exactly in the configuration recommended against
ransomware, deleting an envelope is not revocation, and the revoked key still
opens the silo for anyone who can read that storage. The only real revocation
there is rotating the DEK, which re-wraps it under new envelopes and makes
every old one useless.

SilentSilo does not paper over this. A target marked append-only is never
sent the delete, and the app reports which target withheld it rather than
saying the key was revoked. Setup guidance is in [STORAGE.md](STORAGE.md).

Rotating the vault key is implemented, and on any target that accepts writes
it is a real revocation: every object in storage is re-sealed under the new
key, so a security key that was not carried through stops opening the silo
even though no content was re-encrypted. Content is under per-blob keys
wrapped by a content KEK, and re-wrapping that one key is the whole of the
work, which is why the size of the silo does not matter.

Two limits of that sentence, stated because they decide what it is worth.
On an append-only target the old objects cannot be overwritten, so they keep
their old envelopes and stay readable with the old key; the app names those
targets in the result rather than reporting a clean sweep. And rotation
protects what an attacker has not already taken: anyone who copied the
ciphertext while they had the key still holds it, which no key change
anywhere can undo.

Blob garbage collection and log compaction are implemented too. The
collector runs only from a device holding the whole log, and deletes only
what was unreferenced on the previous pass as well, because a blob reaches
storage before the record that names it and deleting on first sight would
remove content that is about to be referenced.

## Local metadata (`vault.db`)

`vault.db` holds folder/file names, blob references and the password store, in an ordinary (unencrypted) SQLite file. It's a **working copy only**: present on disk while a session is open, deleted on lock or app exit.

At rest, the vault's state lives in `vault.db.enc`: the whole `vault.db` file in one sealed envelope under the Master DEK, single shot, no chunking. Written through a temporary file and a rename, so a crash mid-write cannot leave a half-encrypted snapshot where the vault used to be. Unlocking decrypts `vault.db.enc` into the working copy; locking re-encrypts the working copy and deletes the plaintext.

A local shadow copy, `vault.db.enc.bak`, is refreshed alongside the primary snapshot and used to recover if the primary is missing or fails to decrypt (wrong key, truncated write, tampering). Only `vault.db.enc` is ever written as a backup, never the plaintext working copy.

This was originally intended to be SQLCipher (page-level encryption baked into SQLite itself), but `bundled-sqlcipher`/`bundled-sqlcipher-vendored-openssl` need a working C/OpenSSL build toolchain that isn't reliably available out of the box on Windows. The AES-256-GCM approach reuses the same dependency-free primitive as blob encryption and builds identically on every platform.

### The password store

Logins live as rows in `vault.db` rather than as a file in the tree. Each entry, including its TOTP secret, is one row.

Unlike the rest of the index, an entry is **sealed individually** with the Master DEK (AES-256-GCM, random nonce, base64 in the column) rather than relying only on the encryption of the database as a whole. The reason is the working copy: while a silo is unlocked, `vault.db` exists as plaintext on the local disk. That is a reasonable place for file names and not one for credentials, so anything reading that file, a backup agent sweeping the working directory or a crash leaving it behind, finds ciphertext. Entries are plaintext only in process memory, and only while the panel holds them.

| Where | Password entries |
|-------|------------------|
| `vault.db` working copy (unlocked) | Sealed under the Master DEK |
| `vault.db.enc` at rest | Sealed, inside the encrypted database |
| Operation records in storage | Sealed, then sealed again by sync |
| Process memory, panel open | Plaintext |

Changes travel as `UpsertPassword` / `DeletePassword` operation records carrying the already-sealed entry, so every device stores identical bytes and replay is a pure copy. Sync therefore merges per entry: two devices adding different logins offline both keep them.

One consequence to know: replacing a password writes a new record, and the previous value remains in the operation log until log compaction removes records below a snapshot horizon. See [What deleting does not remove](#what-deleting-does-not-remove).

## Malware on the user's machine

Code running as the user, while a silo is unlocked, can read this process's memory, and the Master DEK has to be there for anything to decrypt at all. No user-space application defends against that, and this one does not claim to. What it does is remove the cheap paths and shorten the window.

| Measure | What it stops |
|---------|---------------|
| Locking on workstation lock, disconnect and suspend | The common case: the user locks the screen and leaves, while the idle timer still has minutes to run |
| Per-entry sealing of passwords | Reading credentials out of the plaintext `vault.db` working copy without touching the process |
| Clipboard opt-out (`CanIncludeInClipboardHistory`, `ExcludeClipboardContentFromMonitorProcessing`) plus a 45-second clear | Copied passwords being retained by Clipboard History, which persists to disk, and by Cloud Clipboard, which syncs them off the machine |
| `ProcessExtensionPointDisablePolicy` | AppInit_DLLs, `SetWindowsHookEx` and other injection paths the system performs on an attacker's behalf, needing no exploit |

One mitigation is deliberately absent. `ProcessSignaturePolicy` with
MicrosoftSignedOnly refuses every non-Microsoft DLL loaded into the process,
and the file dialogs run in this process: any network provider registered on
the machine is loaded by `mpr.dll` into anything that asks about a network
path, and shell extensions arrive the same way. Windows refuses those with
STATUS_INVALID_IMAGE_HASH and shows "Bad Image" once per process, which lands
on the screen where a new user chooses where their silo goes, and leaves the
file picker quietly missing those integrations afterwards. What it bought was
thin: it names a DLL dropped beside the executable, but the install is
per-user, so whoever can drop one there can replace the executable instead,
against which no policy a process sets for itself does anything. Search-order
hijacking from a writable directory was the rest, and `SafeDllSearchMode`
already covers it.

The remaining path worth naming is a **tampered binary**, which can wait for the next unlock and take the key. Nothing inside the program can detect that reliably. Releases are code-signed, which ties a download to its publisher; reproducible builds are the other half, so that a user can check the binary against the published source, and they are not done.

Deliberately not attempted: anti-debugging, obfuscation and similar. They add real complexity, stop nobody who is serious, and make no sense in a project whose source is published.

## Test vectors

Run blob round-trip tests:

```bash
cd silentsilo.gui
cargo test -p silentsilo-crypto roundtrip_small_file
```

Expected: encrypt `hello silentsilo` → decrypt matches; header `content_hash` stable for same plaintext.

A stronger check is the compatibility suite, which rebuilds a silo committed by
a past release from its storage using only a recovery code, and compares the
result against what that release produced:

```bash
cargo test -p silentsilo-fixture
```

Every format on this page is exercised by it. See [FORMATS.md](../FORMATS.md)
for what is versioned and what an older build does when it meets a newer
one.

## What the storage provider learns

Encryption hides the contents, not the shape. A provider, or anyone who
obtains a copy of the bucket, reads all of the following without any key:

| Object | What it reveals |
|--------|-----------------|
| `vault.json` | The vault id, which is random and says nothing else |
| `ops/<lamport>-<device_id>-<op_id>.op` | How many operations exist, their order, when they were made, and how many devices the vault has, from the device ids in the key names |
| An operation body | Its length, which bounds the size of the file name inside and hints at the kind of operation |
| `blobs/<uuid>.sslo` | How many files there are and the size of each, to within a fraction of a percent, since there is no padding |
| `keys/<credential_id>` | The credential id, the public key, the slot, whether it is a platform authenticator, and **the label the user typed**, all in the clear around the wrapped DEK |
| `recovery.env` | That a recovery code exists, and when it was created |

None of this breaks the encryption, and none of it is unusual for a design
that stores one object per change. It is listed because "the provider sees
opaque blobs" is not the whole truth, and because a label like "Alex's work
YubiKey" is the user's own words leaving their machine unencrypted.

Reducing the exposure is possible and is not done today: sealing labels under
the DEK, using a per-pass pseudonym instead of a stable device id, and padding
blobs to a fixed granularity. Each costs something, none is free, and none is
implemented in v1.

Anyone using plain HTTP for S3 or WebDAV also exposes the access key and the
signature on the wire, on top of everything above. Use TLS.

## Zeroization

Everything that can open the vault is wiped when it goes out of scope:

- `MasterDek`, the key everything is encrypted under.
- `VaultKey`, derived from the device secret, and `UnlockMaterial.wrap_key`,
  derived from a security key's `hmac-secret` output. Either unwraps
  `master.dek.enc` on its own.
- The device secret itself, in `LocalVaultAuth`.
- The buffer `unwrap_dek_bytes` unseals the DEK into on its way to `MasterDek`,
  and the decrypted index `decrypt_vault_file` holds while writing the working
  copy. That second one is the whole tree of names.

A test asserts each of those types still carries `ZeroizeOnDrop`, because what
happens in practice is a derive disappearing during a refactor, not someone
deciding to remove it.

What is not covered: plaintext chunk buffers are dropped after write without
an explicit wipe, and no platform here guarantees the pages never reached
swap.

Zeroization is a hardening measure against memory disclosure, not a defence
against code running as the user: see [Malware on the user's
machine](#malware-on-the-users-machine).

## Out of scope (v1)

- Post-quantum algorithms
- Rotation without a security-key touch. Rotating asks for a touch from each
  credential carried through, which is what proves the person doing it still
  holds them.
- Rotation on a target that refuses overwrites. The objects in an append-only
  bucket keep their old envelopes, so a revoked key still opens the copies
  someone already took. `docs/STORAGE.md` says which targets behave this way.
- Requiring user verification (PIN or biometric) at unlock. Ceremonies ask for
  it as `discouraged` today, which is a UX choice with the consequence
  described in the threat model.
- A shared revocation record, so a removed key converges across every device
  rather than only the one it was removed on.
- Padding, label encryption and device-id pseudonyms, which are what would
  narrow "What the storage provider learns".

## Related files

| Component | Path |
|-----------|------|
| Blob encrypt/decrypt | `src-tauri/crates/silentsilo-crypto/src/blob.rs` |
| Sealed payload envelope | `src-tauri/crates/silentsilo-crypto/src/sealed.rs` |
| Master DEK | `src-tauri/crates/silentsilo-crypto/src/dek.rs` |
| DEK wrapping | `src-tauri/crates/silentsilo-vault/src/dek_store.rs` |
| Recovery code | `src-tauri/crates/silentsilo-vault/src/recovery.rs` |
| Key derivation parameters | `src-tauri/crates/silentsilo-vault/src/kdf.rs` |
| Settings file envelope | `src-tauri/crates/silentsilo-vault/src/format.rs` |
| Format versions and rules | `FORMATS.md` |
| DEK wrap | `src-tauri/crates/silentsilo-vault/src/dek_store.rs` |
| vault.db at-rest encryption | `src-tauri/crates/silentsilo-vault/src/vault_file_crypto.rs` |
| Argon2 KDF | `src-tauri/crates/silentsilo-vault/src/kdf.rs` |
| FIDO hmac-secret (Windows) | `src-tauri/crates/silentsilo-fido/src/backend/win.rs` |
| FIDO hmac-secret (Linux/macOS) | `src-tauri/crates/silentsilo-fido/src/backend/ctap.rs` |
