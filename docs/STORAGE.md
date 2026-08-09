# Setting up storage that survives a bad day

SilentSilo backs up to storage you own. Any S3-compatible bucket, WebDAV
share, SFTP account or plain folder works, and nothing here is required to
use the app. This page is about the setup that holds up when something goes
wrong: a drive fails, a laptop is stolen, ransomware runs as you.

The rule worth keeping is the old one. Three copies of anything you care
about, on two different kinds of storage, one of them somewhere else. The
Copies panel in Backup shows where each of yours stands.

## What counts as a copy

A device that holds only the file list is not a copy. By default SilentSilo
syncs names and structure, and fetches a file's contents when you open it, so
a second computer is an index that can produce any file on demand. That is
usually what you want, and it is not a copy of your data. Turn on "Keep a
full copy on this computer" if you intend to count it as one.

A copy the same credentials can erase is not an independent copy either.
Ransomware that owns your machine owns your bucket keys. That is what the
append-only role is for.

## Target roles

Each place you back up to is either a working target or an append-only one.

**Working** is the default and the ordinary case. SilentSilo keeps it tidy:
emptying the trash removes content, unreferenced content is swept, and old
history is pruned after compaction. The storage stays roughly the size of
your silo.

**Append-only** means SilentSilo never sends it a delete. Not when you empty
the trash, not when it sweeps, not when it compacts, not when you revoke a
security key. It grows for ever, and you pay for that. In exchange, nothing
running on your computer can shrink it through the app.

That last phrase is the whole limit of the setting: it is a promise the app
keeps, not a rule the storage enforces. Making the storage enforce it is the
next section for a bucket, and is not possible on a plain folder, which the
external drives section explains.

Tick "Never delete anything here" when adding a place to make it append-only.

## Making a bucket that refuses deletes

The role is a promise SilentSilo makes. It does not stop anything else with
your credentials. Two ways to make the storage enforce it, best first:

**Object lock.** On AWS S3 and several compatible providers, object lock
refuses a delete until the retention date passes, and in compliance mode not
even the account root can override it. It requires versioning, and on AWS it
normally has to be enabled when the bucket is created, so it is a setup step
rather than something the app can turn on for you.

Governance mode can be bypassed by someone with the right permission.
Compliance mode cannot be bypassed by anyone until retention expires, which
includes you when you genuinely want something gone. Choose knowing that.

**Credentials without delete permission.** An IAM policy granting
`s3:PutObject`, `s3:GetObject` and `s3:ListBucket` but not `s3:DeleteObject`
gets you most of the protection with none of the setup. SilentSilo treats a
refused delete as an expected answer rather than an error.

When you tick "Never delete anything here", SilentSilo asks the bucket what
it has and tells you if the answer is weaker than you might assume.

## Versioning alone is not protection, and it breaks revocation

This one is worth reading twice.

On a versioned bucket, a DELETE does not remove anything. It writes a delete
marker and leaves the previous version in place, readable by anyone holding
`s3:GetObjectVersion`.

Most of what SilentSilo writes is unaffected, because operation records and
content blobs go to unique keys and are never overwritten. Two things are
affected, and they are the two that matter:

- **Revoking a security key** deletes that key's envelope, which is what lets
  the key unlock the silo from any device. On a versioned bucket the old
  version stays readable, so the key still works for anyone who can read
  versions.
- **Turning off the recovery code** works the same way.

So on a versioned or object-locked target, deleting an envelope is not
revocation. The only real revocation there is changing the silo's encryption
key, under Settings, Security keys. SilentSilo tells you which target withheld
a deletion rather than reporting success.

Changing the key re-seals every object on the targets that accept writes and
re-wraps the key under only the security keys you carry through, so a removed
key stops opening the silo. Your files are not re-encrypted and nothing is
re-uploaded, whatever the size of the silo.

It cannot reach an append-only target, because its objects cannot be
overwritten: what is already there keeps its old envelope and stays readable
with the old key. The result names those targets rather than claiming a clean
sweep. If that matters to you, the answer is a target with a retention window
short enough that the exposure ends, or credentials without delete permission
on a bucket that does accept overwrites.

## Lifecycle rules: one setting can delete your archive

Object lock only forbids deletion until a date. When retention expires
nothing is removed, it merely becomes removable, and it stays there for ever
unless a lifecycle rule cleans it up. "Locked for three months" does not mean
"clean after three months".

If you add lifecycle rules, use only these:

- `NoncurrentVersionExpiration`, which removes superseded versions.
- `ExpiredObjectDeleteMarker`, which removes delete markers with nothing
  behind them.

**Never put a plain `Expiration` rule on a bucket holding a silo.** Blobs and
operation records are never overwritten, which makes them permanently current
versions. A rule expiring current versions deletes the live archive, not the
rubbish. In the AWS console the difference is one checkbox.

## External drives and plain folders

A folder on an external drive is a perfectly good copy, and it is the second
medium most people already own. What it cannot be is enforced append-only,
and the reason is worth knowing before you try.

Object lock has no equivalent on a plain filesystem. The obvious substitute
is a permission that refuses deletion, on Windows through an ACL, on Linux
through `chattr +a`. **It does not work with a folder target**, and not
because the app objects: SilentSilo writes each object beside its final name
and renames it into place, so that a sync client watching the folder never
sees a half-written object and uploads the fragment. Renaming needs the
right to delete the source, so a permission that forbids deletion forbids
writing too, and the backup stops rather than becoming immutable.

Ticking "Never delete anything here" still works and is still worth doing.
It stops SilentSilo from sending a delete, which covers the emptied trash
and the bug in our code. It does not stop anything else on the machine, and
on a folder there is nothing underneath to enforce it.

What does work on a local drive, strongest first:

**Unplug it.** A disconnected disk cannot be encrypted, deleted or ruined by
anything running on your computer. It is the oldest answer and it is still
the best one available here. Plug it in, let the sync finish, unplug it.

**A NAS instead of a bare disk.** Snapshots on ZFS or btrfs, or the
write-once folders some NAS operating systems offer, are enforced by the
device rather than by the machine writing to it. Deleting from the share
does not touch the snapshot. This is the closest thing to object lock that
lives in a house.

**Write-once media.** BD-R discs are genuinely write-once, and LTO tape sold
as WORM cannot be rewritten at all. Both are impractical for a whole silo of
any size and excellent for a small unchanging set: the emergency kit, the
handful of documents that would actually hurt to lose.

A second drive you rotate weekly, kept unplugged between times, beats an
always-connected one with clever permissions. The threat that takes your
files is running as you, with your rights, on your machine.

## Cold storage

Glacier and similar classes are not supported. Retrieval takes hours and the
sync pass would fail in a way that reads as a broken bucket rather than as
"this is thawing". Use standard or infrequent-access classes. If you want an
archive tier, use a separate append-only target on a provider whose cheap
tier is still immediately readable.

## A setup that works

One reasonable arrangement, for a silo that matters:

1. Your computer, with "Keep a full copy" on. Copy one.
2. A working target on a bucket or NAS. Copy two, different medium, and the
   one the app keeps tidy.
3. An append-only target on a different provider, with object lock or
   delete-free credentials. Copy three, somewhere else, and the one that
   survives your machine being taken over.

If a bucket is not something you want to arrange, an external drive that
lives unplugged does the same job as copy three. It is offline rather than
immutable, which stops the same attack for a different reason, and it costs
one habit instead of one subscription.

Check the Copies panel now and again. It shows when each was last written to,
which is the fact that tells you a disk has been unplugged since spring.
