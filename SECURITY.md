# Security Policy

## Reporting a vulnerability

Report vulnerabilities privately to **security@silentsilo.com**. Please do
not open a public issue for anything that could be a vulnerability:
anything touching `silentsilo-crypto`, `silentsilo-vault`, key material,
the FIDO2 flows or the recovery code.

This is a one-person project, so reports are read by one person: the aim is
an acknowledgement within 72 hours, and you will get one as soon as I see it.
Please include steps to reproduce and the commit or release version you
tested against.

## Scope

SilentSilo is local-first and zero-knowledge by design: there is no server
and no account. A storage provider holding a silo's backup sees ciphertext,
plus one small manifest naming a random vault id.

Four things reach the network, and nothing else does. The first is the
backup storage the user configures, which is optional and carries only the
ciphertext described in `docs/CRYPTO.md`. The second is the updater, which
asks `releases.silentsilo.com` whether a newer version exists and downloads
it from GitHub; update packages are signed, and the app verifies that
signature before installing. Automatic update checks can be turned off in
Settings. The third is the breach check in Health, which runs only when its
button is pressed: the first five characters of each password's SHA-1 go to
`api.pwnedpasswords.com` (the k-anonymity range API, with padded responses),
the passwords themselves never leave, and nothing identifying the user or
the silo goes with the request. The fourth is site icons in the credentials
list, off unless switched on: each is fetched as `/favicon.ico` from the host
named in the entry itself, never through an icon service that would see every
site at once, and private, local and non-public hostnames are skipped so the
fetch cannot be turned into a probe of the user's own network. Switching it on
tells each of those sites that someone holding an entry for them opened this
list, which is why it is off to begin with.

The threat model, key hierarchy and on-disk formats are documented in
[`docs/CRYPTO.md`](docs/CRYPTO.md). That document is the reference for what
this project does and does not defend against.

One scope note about organisation-administered silos, since it is the one place
where the app enforces a rule against the person at the keyboard rather than for
them. A silo created that way carries keys its user cannot retire, and the
enforcement is in the client, not in the cryptography: those keys unwrap the
same vault key as any other, and someone editing the silo's files by hand or
running a modified build can clear the marking on their own disk. That is not a
vulnerability, it is the design. What actually holds is the copy the
organisation keeps on storage it owns. Reports that the marking can be bypassed
locally are welcome as documentation bugs if the docs overstate it, not as
security issues.

[`FORMATS.md`](FORMATS.md) lists every persisted format with its version and
what an older build does when it meets a newer one. A format change that could
leave a vault unopenable is treated as a security issue, not a compatibility
one: the data is unrecoverable by the person who owns it.

## Supported versions

Only the latest release receives fixes. Every release is free and complete,
so a security patch reaches everyone the same way.
