# Provisioning silos for an organisation

A company setting silos up for its people has one problem an individual does
not: the archive has to survive the person leaving. This document is the
procedure for that, written for whoever runs IT. The employee-facing summary is
on the website's security page; the format-level rules are in
[FORMATS.md](https://github.com/silentsilo/core/blob/main/FORMATS.md), and the honest limits of the whole mechanism are at
the end of this page.

## What the feature is

A silo can be created as **administered by an organisation**. It is a checkbox
on the screen where the first key is enrolled, off by default, and it exists
only there: a silo already in use can never be converted. The key enrolled with
it becomes an organisation key. Whoever uses the silo afterwards cannot retire
that key, and cannot regenerate or disable the recovery code, without one of the
organisation's keys present and verified.

An organisation key decrypts nothing more than any other key. The whole
difference is who may administer the silo, not who may read it.

### Where the marking holds

The marking is a field on the key envelope, and since core 1.6.0 a device
joining a silo does not trust that field as it finds it in storage: anyone
who can write to the storage could plant an `org` mark and lock that device
out of key changes and recovery-code changes for good. A join keeps `org`
only on the key that opened the join, because that key proved itself by
unwrapping the DEK. A recovery-code join keeps it on no key at all.

So the in-app rule holds on the machine where the silo was created, and on
any machine set up from the backup with an organisation key itself. A
machine set up with the employee's own key, or with the recovery code,
lists the company key as an ordinary key: the employee can remove it there,
and that removal reaches every working target. The app does not send it to
a never-delete copy, which is why the company's copy matters (see the last
section) and why the remote onboarding flows below depend on it. "Never
deletes" is what the app does, not what the storage allows: an employee who
can delete files on that storage by hand can still remove the company's key
there. The storage has to refuse it, as the next section sets up.

## The layout that works

- **One silo per employee.** A shared silo has no per-person access control,
  so one silo per person is the unit that matches offboarding.
- **The silo lives on the employee's machine.** The working copy is local by
  design; do not put it on a share.
- **Backup goes to a per-employee folder on storage the company controls**,
  for example `\\server\vaults\popescu`, with permissions restricted to that
  employee and IT. Any of the four backends works; a share is the simplest.
- **A second place, a never-delete copy, on storage the company controls
  and the employee cannot delete from.** "Never-delete copy" is offered only
  under Settings, Backup, when adding another copy; the main copy, at the top
  of that page, is always the working one. The tick only stops the app from
  deleting. The storage must refuse deletes too: a share where the employee's
  account may create and write files but not delete them, an S3 bucket with
  object lock, or sign-in details with no delete permission. This copy is the
  one that survives whatever happens on the employee's machine, so it is not
  optional in this layout.
- **The company holds the recovery code and the organisation keys.** The code
  goes in the safe with the keys. The employee does not get a copy, and does
  not need one: their own enrolled key is their way in, and IT can always let
  them back in.

Enrol **two organisation keys**, not one, and keep them apart. Replacing an
organisation key requires another organisation key, so the company that loses
its only one has permanently lost the ability to administer that silo. The app
will not stop you from provisioning with one; this page is where you are told
not to.

## Onboarding, three ways

**At the desk (preferred).** Create the silo on the employee's machine, tick
the organisation box, enrol the organisation key, set the backup target, add
the never-delete copy under Settings, Backup, and let the first sync finish.
Then enrol the employee's own key in the same unlocked session and hand it to
them. Same ceremony as issuing a badge. This is the only flow in which the
in-app rule holds on the employee's own machine.

**Remote, by shipping a key.** Do the same provisioning at the IT desk, enrol
the employee's key there too, sync, then courier the key to them. On their
machine they choose *Set up from backup storage*, point it at their folder,
and touch the key. No secret ever travels over a digital channel. After the
first unlock they can add Windows Hello themselves; Hello is sealed to their
machine and cannot be pre-enrolled. Their key is not an organisation key, so
on their machine the company key carries no marking (see "Where the marking
holds"): the never-delete copy is what protects the company's way in.

**Remote, by recovery code (last resort).** Send the code, have the employee
join with it and enrol their key **in that same session**, then regenerate the
code at IT, which invalidates the one the employee saw. Three sharp edges:
until the employee enrols a key, the only enrolled key is the company's, so
closing the app mid-onboarding means starting over with the code; until the
code is regenerated, the employee holds something that opens the silo from
anywhere; and a recovery-code join clears the organisation marking on every
key, so on that machine the company key is an ordinary one. Do not skip the
regeneration, and do not send the code over a channel you would not send a
password over.

## Offboarding and break-glass

Take **both** organisation keys out of the safe: the rotation asks for a
touch on every key that is to be kept, and a key from another device or not
plugged in cannot be kept from that machine, so a rotation done with one
company key in hand drops the other. Open the silo from the working target,
on any machine, via *Set up from backup storage*, and use **Replace the
encryption key** under Settings, Advanced, keeping only the two
organisation keys. One operation does most of the job: the former employee's
key stops opening the working target, which moves to the new key, and a fresh
recovery code is shown once for the safe. Merely removing their key is not
enough on storage that keeps what it is asked to delete, which is exactly
what a never-delete company copy does; the app says the same thing on the
replace panel.

Replacing the key does not touch a never-delete copy, and a freshly set-up
machine has only the folder it was set up from, so the company's never-delete
copy keeps the old key file and the former employee's key goes on opening
what was stored there before the change. Remove their access to both company folders the same
day; that is what closes it.

What they already copied while they had access is theirs forever; no design
anywhere undoes that.

Handing a silo over to a new owner is the same flow ending differently: retire
the organisation keys last, and the silo becomes an ordinary personal one.

## What the app enforces, and what the storage has to

The rules are enforced by the app, not by the cryptography. Someone who edits
the silo's files by hand, or runs a modified build, can clear the organisation
marking on their own disk; the licence gives them that ability. The app never
deletes from a **never-delete copy**, but that is the app's behaviour, not a
property of the storage. The company's key file survives there only if the
storage itself refuses the employee's deletes: permissions that allow writing
but not deleting, object lock, or sign-in details without delete rights.
Treat the in-app rules as what keeps honest people honest, and a company copy
the employee cannot delete from as what holds.

Nothing here is a way to read an employee's silo without a key. The company
can open the silo because it enrolled a key at creation, not because a
mechanism exists to bypass one.
