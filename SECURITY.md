# Security Policy

## Reporting a vulnerability

Report vulnerabilities privately to **security@silentsilo.com**. Please do
not open a public issue for anything that could be a vulnerability:
anything touching `silentsilo-crypto`, `silentsilo-vault`, key material,
the FIDO2 flows or the recovery code.

You will get an acknowledgement within 72 hours. Please include steps to
reproduce and the commit or release version you tested against.

## Scope

SilentSilo is local-first and zero-knowledge by design: there is no server
and no account. A storage provider holding a silo's backup sees ciphertext,
plus one small manifest naming a random vault id.

Three things reach the network, and nothing else does. The first is the
backup storage the user configures, which is optional and carries only the
ciphertext described in `docs/CRYPTO.md`. The second is the updater, which
asks `releases.silentsilo.com` whether a newer version exists and downloads
it from GitHub; update packages are signed, and the app verifies that
signature before installing. Automatic update checks can be turned off in
Settings. The third is the hosted storage service at
`storage.silentsilo.com`, reached only if the user chooses to connect a silo
to it: the app shows a short code, the person approves it in a browser, and
the storage credentials come back to the app. After that the app talks
straight to the storage provider, so a silo already connected keeps working
whether or not the service is reachable. A silo that never uses it never
contacts it.

The threat model, key hierarchy and on-disk formats are documented in
[`docs/CRYPTO.md`](docs/CRYPTO.md). That document is the reference for what
this project does and does not defend against.

[`FORMATS.md`](FORMATS.md) lists every persisted format with its version and
what an older build does when it meets a newer one. A format change that could
leave a vault unopenable is treated as a security issue, not a compatibility
one: the data is unrecoverable by the person who owns it.

## Supported versions

Only the latest release receives fixes. Every release is free and complete,
so a security patch reaches everyone the same way.
