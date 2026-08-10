# Provisioning silos for an organisation

A company setting silos up for its people has one problem an individual does
not: the archive has to survive the person leaving. This document is the
procedure for that, written for whoever runs IT. The employee-facing summary is
on the website's security page; the format-level rules are in
[FORMATS.md](../FORMATS.md), and the honest limits of the whole mechanism are at
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

## The layout that works

- **One silo per employee.** A shared silo has no per-person access control,
  so one silo per person is the unit that matches offboarding.
- **The silo lives on the employee's machine.** The working copy is local by
  design; do not put it on a share.
- **Backup goes to a per-employee folder on storage the company controls**,
  for example `\\server\vaults\popescu`, with permissions restricted to that
  employee and IT. Any of the four backends works; a share is the simplest.
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
the organisation box, enrol the organisation key, set the backup target, and
let the first sync finish. Then enrol the employee's own key in the same
unlocked session and hand it to them. Same ceremony as issuing a badge.

**Remote, by shipping a key.** Do the same provisioning at the IT desk, enrol
the employee's key there too, sync, then courier the key to them. On their
machine they choose *Copy one from backup storage*, point it at their folder,
and touch the key. No secret ever travels over a digital channel. After the
first unlock they can add Windows Hello themselves; Hello is sealed to their
machine and cannot be pre-enrolled.

**Remote, by recovery code (last resort).** Send the code, have the employee
join with it and enrol their key **in that same session**, then regenerate the
code at IT, which invalidates the one the employee saw. Two sharp edges: until
the employee enrols a key, the only enrolled key is the company's, so closing
the app mid-onboarding means starting over with the code; and until the code
is regenerated, the employee holds something that opens the silo from
anywhere. Do not skip the regeneration, and do not send the code over a
channel you would not send a password over.

## Offboarding and break-glass

Take an organisation key out of the safe, open the silo (from the backup, on
any machine, via *Copy one from backup storage*), and use **Change the silo's
encryption key** in Settings, keeping only the keys that should survive. One
operation does the whole job: the former employee's key stops opening
anything, storage is re-sealed under the new key, and a fresh recovery code
comes out for the safe. Merely removing their key is not enough on storage
that keeps what it is asked to delete, which is exactly what an append-only
company target does; the app says the same thing on the rotation panel.

What they already copied while they had access is theirs forever; no design
anywhere undoes that.

Handing a silo over to a new owner is the same flow ending differently: retire
the organisation keys last, and the silo becomes an ordinary personal one.

## What this does and does not guarantee

The rules are enforced by the app, not by the cryptography. Someone who edits
the silo's files by hand, or runs a modified build, can clear the organisation
marking on their own disk; the licence guarantees them that ability. What they
cannot do is remove the company's key envelope from a backup target the
company owns and has marked **append-only**. That copy is the actual
guarantee, which is why the backup target belongs on company storage and why
the append-only role exists. Treat the in-app rules as what keeps honest
people honest, and the company-held copy as what holds.

Nothing here is a way to read an employee's silo without a key. The company
can open the silo because it enrolled a key at creation, not because a
mechanism exists to bypass one.
