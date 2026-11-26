# Security Policy

## Reporting a vulnerability

Report vulnerabilities privately to **security@silentsilo.com**, or with
**Report a vulnerability** under this repository's Security tab. Please do
not open a public issue for anything that could be a vulnerability: anything
touching key material, the FIDO2 flows, the recovery code, or what a silo
writes to disk.

The same address covers
[silentsilo/core](https://github.com/silentsilo/core), where the
cryptography, the persisted formats and sync live. Report to whichever
repository you found it in; it reaches the same person either way.

This is a one-person project, so reports are read by one person: the aim is
an acknowledgement within 72 hours, and you will get one as soon as I see it.
Please include steps to reproduce and the commit or release version you
tested against.

## Scope

SilentSilo is local-first by design: there is no SilentSilo server and no
SilentSilo account. A storage provider holding a silo's backup gets the
encrypted silo; what it can still read without a key is listed in the
cryptography specification.

Four things reach the network. The first is the backup storage the user
configures, which is optional and carries only the ciphertext described in
the cryptography specification. For OneDrive, Dropbox and Google Drive it
also takes the provider's sign-in, done in the user's browser with PKCE, the
token refresh, and a read of the account's address and free space. The app
asks only for its own folder at each (`Files.ReadWrite.AppFolder`,
Dropbox's app folder, `drive.file`), and the refresh token stays on the
computer. The second is the
update check, which reads `silentsilo.com/latest.json` (or the same file in
the website's GitHub repository when the site does not answer) to learn the
latest version number, with nothing about the install in the request. The
app never downloads or installs an update: it opens the download page and
the user installs the new version, a deliberate choice explained in
`docs/ARCHITECTURE.md`, "Updates". Automatic update checks can be turned off
in Settings. The third is the breach check in Health, which runs only when its
button is pressed: the first five characters of each password's SHA-1 go to
`api.pwnedpasswords.com` (the k-anonymity range API, with padded responses),
the passwords themselves never leave, and nothing identifying the user or
the silo goes with the request. The fourth is site icons in the credentials
list, off unless switched on: each is fetched as `/favicon.ico` from the host
named in the entry itself, never through an icon service that would see every
site at once, and private, local and non-public hostnames are skipped so the
fetch cannot be turned into a probe of the user's own network. The app makes
these requests itself, over HTTPS only, to the address it checked, and keeps
the icons in memory: the window's disk cache holds no list of sites. Switching it on
tells each of those sites that someone holding an entry for them opened this
list, which is why it is off to begin with.

The threat model, key hierarchy and on-disk formats are documented in
[`docs/CRYPTO.md`](https://github.com/silentsilo/core/blob/main/docs/CRYPTO.md). That document is the reference for what
this project does and does not defend against.

Organisation-administered silos: the rule that a user cannot retire the
organisation's keys is enforced by the app, not by the cryptography, so
editing the silo's files or running a modified build clears it locally.
Reports of that are documentation bugs if the docs overstate it, not
security issues. What holds is the organisation's own copy; see
[`docs/ORGANISATIONS.md`](docs/ORGANISATIONS.md).

[`FORMATS.md`](https://github.com/silentsilo/core/blob/main/FORMATS.md) lists every persisted format with its version and
what an older build does when it meets a newer one. A format change that could
leave a vault unopenable is treated as a security issue, not a compatibility
one: the data is unrecoverable by the person who owns it.

## Verifying a download

From the release after 1.5.2, every release on GitHub carries `SHA256SUMS`,
the SHA-256 of each of its files, and `SHA256SUMS.minisig`, its
[minisign](https://jedisct1.github.io/minisign/) signature. The public key
is [`release-checksums.pub`](release-checksums.pub) in this repository, and
goes on silentsilo.com with the first release that carries the lists:

```
RWRAWSxOMH1Dl6fHrVvTKc2NzrYkwMhW5UvPR96mKXLjyHUnTzY0xtvO
```

With the files downloaded into one folder:

```
minisign -Vm SHA256SUMS -P RWRAWSxOMH1Dl6fHrVvTKc2NzrYkwMhW5UvPR96mKXLjyHUnTzY0xtvO
sha256sum -c --ignore-missing SHA256SUMS
```

On Windows, `Get-FileHash <file>` gives the hash to compare with its line in
`SHA256SUMS`. Windows and macOS also check the installer's own signature
(Authenticode, Developer ID) when it runs; on Linux this is the check there
is. The key signs only these lists. The app carries no copy of it, and it
installs nothing.

## Supported versions

Only the latest release receives fixes. Every release is free and complete,
so a security patch reaches everyone the same way.
