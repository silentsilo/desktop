# Builds and signs, on this machine, the release artifacts CI cannot: the
# installer and the Windows extractor, the updater signature of every file
# on the release, and latest.json. The Authenticode certificate sits on a
# hardware token no runner reaches, and the updater key is on no server at
# all, which is the whole reason they are made here.
#
# Before running it: the release workflow has attached the Linux app, the
# Linux extractor and the SBOM to the draft, and
# scripts/build-release-mac.sh has attached the signed macOS app and the
# macOS extractor from the Mac. This script checks the provenance of the CI
# files, then signs them all for the updater.
#
# The Linux extractor is built by this repository's release workflow, the
# macOS one by the Mac script, each from a clean clone of the core tag this
# build pins, and attached to the same draft. Core publishes tags and no releases, so there is nothing
# to download from it. They are not built here on purpose: a binary out of
# WSL links against this machine's glibc, and the extractor is the one tool
# that has to start on a machine nothing is assumed about.
#
# The Windows extractor is built here, from a clean clone of that same core
# tag, because it needs the Authenticode signature.
#
# Run it from the repo root:
#
#   .\scripts\build-release-local.ps1
#
# Two different signatures go on: Authenticode, which tells Windows who
# published the installer, and minisign, which is what the updater checks
# before installing anything. They are unrelated and both are required.
#
# Authenticode needs the hardware token, asked for only after everything is
# compiled: dependency code runs while compiling, and a token that is not
# plugged in signs nothing for it. The updater key's password is asked for
# once, at the end, by tools\release-signer, which is built from its own
# lockfile before anything of the app's and checked by hash before it runs:
# `tauri signer` runs out of node_modules, which any npm package can rewrite.
#
# Everything lands in dist-release\, ready to upload to the GitHub release.

$ErrorActionPreference = "Stop"

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$version = (Get-Content package.json -Raw | ConvertFrom-Json).version
$tag = "v$version"
$slug = "silentsilo/desktop"
$keyFile = Join-Path $HOME ".tauri\silentsilo.key"
$out = Join-Path $repoRoot "dist-release"

Write-Host "Building SilentSilo $version" -ForegroundColor Cyan

if (-not (Test-Path $keyFile)) {
    throw "Updater signing key not found at $keyFile"
}

# Google's token endpoint wants the desktop client's secret. Google calls it
# not confidential and it ends up inside the binary, but it stays out of the
# repositories. Core reads it at compile time; a build without it has no
# Google Drive, so a release refuses to go without it.
$googleSecretFile = Join-Path $HOME ".silentsilo-release\google-client-secret.txt"
if (-not $env:SILENTSILO_GOOGLE_CLIENT_SECRET) {
    if (-not (Test-Path $googleSecretFile)) {
        throw "Google's client secret is not at $googleSecretFile. Without it this build has no Google Drive."
    }
    $env:SILENTSILO_GOOGLE_CLIENT_SECRET = (Get-Content $googleSecretFile -Raw).Trim()
}

# The three version-bearing files must agree, or the updater compares one
# version while the binary reports another. Cheap to check, expensive to miss.
$confVersion = (Get-Content src-tauri\tauri.conf.json -Raw | ConvertFrom-Json).version
if ($confVersion -ne $version) {
    throw "Version mismatch: package.json says $version, tauri.conf.json says $confVersion"
}
$cargoVersion = ([regex]'(?m)^version\s*=\s*"([^"]+)"').Match(
    (Get-Content src-tauri\Cargo.toml -Raw)).Groups[1].Value
if ($cargoVersion -ne $version) {
    throw "Version mismatch: package.json says $version, Cargo.toml says $cargoVersion"
}
# The workspace's own version, which silentsilo-shell and the browser host
# take theirs from.
$workspaceVersion = ([regex]'(?m)^version\s*=\s*"([^"]+)"').Match((Get-Content Cargo.toml -Raw)).Groups[1].Value
if ($workspaceVersion -ne $version) {
    throw "Version mismatch: package.json says $version, the workspace Cargo.toml says $workspaceVersion"
}

# A `[patch]` pointing Cargo at a local core checkout rewrites Cargo.lock and
# builds the app against whatever sits in a sibling directory rather than
# against the pinned tag. Convenient while developing, catastrophic in a
# signed installer that nobody can trace back to a revision.
if (Test-Path (Join-Path $repoRoot ".cargo\config.toml")) {
    throw ".cargo\config.toml exists. It patches the core crates to a local checkout; remove it, run cargo check to restore Cargo.lock, then build."
}

# The browser host lets in the extension ids compiled into it. With no store
# id yet (Chrome Web Store, Edge Add-ons or Firefox Add-ons) the release goes
# out without it; an entry that is a development id (the Chromium key is
# public; the Firefox id is not ours until our AMO submission claims it), not
# a plain extension origin, or not a Firefox add-on id stops the build. Checked
# here so a long build does not start for nothing, and again on the built
# host below.
. (Join-Path $PSScriptRoot "browser-host-release.ps1")
$shipHost = (Get-BrowserHostPlan -OriginsPath crates\silentsilo-browser-host\allowed-origins.json `
        -DevPath crates\silentsilo-browser-host\allowed-origins.dev.json) -eq "ship"
if (-not $shipHost) {
    Write-Host "No store id in allowed-origins.json: this release ships without the browser extension host." -ForegroundColor Yellow
}

# Says which core revision this installer will contain, and refuses a lockfile
# that points anywhere but silentsilo/core.
node scripts\check-lockfile.mjs
if (-not $?) { throw "Cargo.lock does not point at silentsilo/core" }

# The notices ship inside the installer and say they were generated from the
# resolved graph, so a lockfile that moved after them makes that sentence
# false: 1.0.0 shipped a file with no OpenSSL and no SQLCipher in it. Commit
# dates rather than file times, because a fresh clone gives every file the
# same mtime. Both changed in one commit is fine. A lockfile that changed
# after is fine only when regenerating gives the same file but for its date
# line: a version bump moves the lockfile and none of the components.
$noticesAt = [int](git log -1 --format=%ct -- THIRD-PARTY-NOTICES.txt)
$lockAt = ('Cargo.lock', 'package-lock.json' | ForEach-Object { [int](git log -1 --format=%ct -- $_) } |
    Measure-Object -Maximum).Maximum
if ($lockAt -gt $noticesAt) {
    $committed = Get-Content THIRD-PARTY-NOTICES.txt | Where-Object { $_ -notmatch '^Generated from the resolved dependency graph on ' }
    node scripts\generate-notices.mjs | Out-Null
    $fresh = Get-Content THIRD-PARTY-NOTICES.txt | Where-Object { $_ -notmatch '^Generated from the resolved dependency graph on ' }
    git checkout -- THIRD-PARTY-NOTICES.txt
    if (Compare-Object $committed $fresh -SyncWindow 0) {
        throw "THIRD-PARTY-NOTICES.txt no longer matches the lockfiles. Run 'npm run notices', commit, retag, then build."
    }
}

# A release build answers for the tag, so it must be built from the tag: a
# dirty tree or a HEAD the tag does not point at produces an installer whose
# source can never be named again, and nothing downstream detects it.
if (git status --porcelain) {
    throw "Working tree is not clean. Commit or stash, then build."
}
if ((git tag --points-at HEAD) -notcontains $tag) {
    throw "HEAD is not tagged $tag. Build from the tagged commit (git switch --detach $tag), or tag this one first."
}

# Every exe this release ships must start on a clean Windows: none may
# import the Visual C++ runtime (vc-runtime.ps1).
. (Join-Path $PSScriptRoot "vc-runtime.ps1")

# No signing key belongs on GitHub: anyone who can push a tag and a changed
# workflow can read a secret there. Checked by name, on the repository and
# each of its environments, before anything is built or signed.
function Assert-NoSigningSecretsOnGitHub {
    param([string]$Repo)
    $pattern = '^(TAURI_SIGNING_|APPLE_)|SIGN|CERTIFICATE|PRIVATE_KEY|KEYSTORE|\bP12\b'
    $found = @()
    $names = gh secret list --repo $Repo --json name --jq '.[].name'
    if (-not $?) { throw "could not list the secrets of $Repo" }
    $found += $names | Where-Object { $_ -match $pattern } | ForEach-Object { "$_ (repository)" }
    $envs = gh api "repos/$Repo/environments" --jq '.environments[].name'
    if (-not $?) { throw "could not list the environments of $Repo" }
    foreach ($env in $envs) {
        $names = gh secret list --repo $Repo --env $env --json name --jq '.[].name'
        if (-not $?) { throw "could not list the secrets of $Repo, environment $env" }
        $found += $names | Where-Object { $_ -match $pattern } | ForEach-Object { "$_ (environment $env)" }
    }
    if ($found) {
        throw "Signing secrets are on GitHub, where a pushed tag can read them: $($found -join ', '). Delete them, then build."
    }
}
Assert-NoSigningSecretsOnGitHub -Repo $slug

# The updater signer, built before npm or cargo touch the app, from its own
# lockfile, and hashed now so the copy that gets the password is this one.
Write-Host "`n== Updater signer ==" -ForegroundColor Cyan
cargo build --release --locked --manifest-path tools\release-signer\Cargo.toml
if (-not $?) { throw "building the updater signer failed" }
$signer = Join-Path $repoRoot "tools\release-signer\target\release\silentsilo-release-signer.exe"
$signerHash = (Get-FileHash $signer -Algorithm SHA256).Hash

# Everything signing needs, found now rather than an hour in: the updater
# key readable, the Authenticode certificate known to this machine (the
# token itself is asked for later), and gh signed in for the draft.
& $signer --key $keyFile --check
if ($LASTEXITCODE -ne 0) { throw "the updater key at $keyFile cannot be read" }
$pubkey = (Get-Content src-tauri\tauri.conf.json -Raw | ConvertFrom-Json).plugins.updater.pubkey
if (-not $pubkey) { throw "tauri.conf.json carries no updater public key" }
$thumbprint = if ($env:SILENTSILO_SIGN_THUMBPRINT) { $env:SILENTSILO_SIGN_THUMBPRINT } else { "5DF397C236D87DA3DB0137E9CBFDF29319241E4A" }
if (-not (Get-ChildItem Cert:\CurrentUser\My | Where-Object { $_.Thumbprint -eq $thumbprint })) {
    throw "The Authenticode certificate $thumbprint is not known to this machine. Plug the token in once with proCertum CardManager closed so Windows registers it."
}
gh auth status 2>&1 | Out-Null
if (-not $?) { throw "gh is not signed in (gh auth login)" }

try {
    # One release per run. Leftovers from an earlier tag would ride along in
    # the `gh release upload dist-release\*` step at the end and attach an
    # old installer to a new release. Emptied rather than recreated, so an
    # Explorer window already open on the folder survives the build.
    New-Item -ItemType Directory -Force -Path $out | Out-Null
    Get-ChildItem $out | Remove-Item -Recurse -Force

    # rust-toolchain.toml pins a version, and an antivirus that quarantines
    # cargo.exe leaves that pin unsatisfiable: rustup cannot reinstall what it
    # is being blocked from writing. Fall back to another installed toolchain,
    # but only one reporting the same rustc version, so the binary is the one
    # the pin asked for and not merely something that compiled.
    if (-not $env:RUSTUP_TOOLCHAIN) {
        $pin = ([regex]'channel\s*=\s*"([^"]+)"').Match(
            (Get-Content rust-toolchain.toml -Raw)).Groups[1].Value
        # Checked on disk, not with `rustup toolchain list`: the list reports
        # a pinned toolchain as active whether or not its files still exist,
        # which is exactly the case being handled here.
        $rustupHome = if ($env:RUSTUP_HOME) { $env:RUSTUP_HOME } else { Join-Path $HOME ".rustup" }
        $pinnedCargo = Join-Path $rustupHome "toolchains\$pin-x86_64-pc-windows-msvc\bin\cargo.exe"
        if (-not (Test-Path $pinnedCargo)) {
            $stable = (& rustc +stable --version) -replace '^rustc (\S+).*', '$1'
            if ($stable -eq $pin) {
                Write-Host "Toolchain $pin is not installed; using stable, which is also $stable." -ForegroundColor Yellow
                $env:RUSTUP_TOOLCHAIN = "stable"
            }
            else {
                throw "Toolchain $pin is not installed and stable is $stable, not the same version"
            }
        }
    }

    # Same gate as CI: a release build must never be the first time the test
    # suite runs.
    Write-Host "`n== Checks ==" -ForegroundColor Cyan
    # The dependencies as the lockfile names them, before any of them runs.
    npm ci; if (-not $?) { throw "npm ci failed" }
    npm run typecheck; if (-not $?) { throw "typecheck failed" }
    npm run lint;      if (-not $?) { throw "lint failed" }
    npm test;          if (-not $?) { throw "tests failed" }
    npm run build;     if (-not $?) { throw "frontend build failed" }
    cargo fmt --all -- --check; if (-not $?) { throw "cargo fmt failed" }
    cargo clippy --all-targets --locked -- -D warnings; if (-not $?) { throw "clippy failed" }
    cargo test --all --locked;  if (-not $?) { throw "cargo test failed" }

    Write-Host "`n== Installer ==" -ForegroundColor Cyan
    # The Authenticode certificate lives on a hardware token, so a GitHub
    # runner cannot reach it and signCommand stays out of tauri.conf.json.
    # Merging it in only here is what lets a tagged build stay green in CI
    # while the release people actually download is signed.
    #
    # tauri.signing.json also turns the updater artifacts off, so the build
    # never sees the updater key or its password: every crate's build script
    # runs in it. The installer gets its updater signature further down, after
    # Authenticode has changed its bytes, so the .sig covers what ships.
    # Direct, not `npm run tauri:build`: npm drops forwarded arguments for a
    # script chained with `&&`, taking the signing config with them.
    npm run icons; if (-not $?) { throw "icon generation failed" }
    # The clean-tree check ran before this, and this rewrites committed
    # files: every PNG, the ICO, the ICNS and the tray template. If the
    # renderer produces different bytes than the ones the tag holds, the
    # installer would carry icons no commit contains. Cheaper to notice here
    # than to explain later.
    if (git status --porcelain -- src-tauri\icons) {
        throw "npm run icons changed committed icons. Commit them, retag, then build."
    }
    # The browser extension's native host ships beside the app as an
    # externalBin, so Tauri signs it with signCommand before packing it and
    # the installer hooks register it. It is merged in here rather than kept
    # in tauri.conf.json because tauri-build insists the file exists on
    # every compile of the app, CI's cargo check included. Copied fresh each
    # time: Tauri skips a sidecar that already carries a signature. Left out
    # entirely while no store id exists (see above).
    $sidecar = "src-tauri\binaries\silentsilo-browser-host-x86_64-pc-windows-msvc.exe"
    $tauriConfigs = @("--config", "src-tauri/tauri.signing.json")
    if ($shipHost) {
        cargo build -p silentsilo-browser-host --release --locked
        if (-not $?) { throw "browser host build failed" }
        # The built host judges itself: no development id, no dev-extension
        # feature, at least one store id.
        & "target\release\silentsilo-browser-host.exe" --check-release
        if ($LASTEXITCODE -ne 0) { throw "the browser host is not fit to ship (see above)" }
        New-Item -ItemType Directory -Force -Path (Split-Path $sidecar) | Out-Null
        Copy-Item "target\release\silentsilo-browser-host.exe" $sidecar -Force
        $tauriConfigs += @("--config", "src-tauri/tauri.browser-host.json")
    }

    # Compiled first, with no token in the machine: every build script runs
    # here. Bundled, and so signed, only once the token is asked for below.
    npx --no-install tauri build --no-bundle @tauriConfigs
    if (-not $?) { throw "tauri build failed" }

    # The extractor lives in silentsilo/core now, so it is built from a clean
    # clone of the tag this app is pinned to, at the exact commit Cargo.lock
    # records. Cloning rather than reusing a sibling working copy: that copy
    # may have uncommitted work, and this binary is the one somebody reaches
    # for when they have lost confidence in everything else.
    #
    # The Linux extractor is built by the release workflow on GitHub and the
    # macOS one on the Mac, from this same core tag. A binary built out of WSL links
    # against this machine's glibc, and the extractor is the one tool that
    # has to start on a machine nothing is assumed about.
    Write-Host "`n== CLI, Windows ==" -ForegroundColor Cyan
    $coreTag = ([regex]'silentsilo-core = \{ git = "[^"]+", tag = "([^"]+)"').Match(
        (Get-Content Cargo.toml -Raw)).Groups[1].Value
    if (-not $coreTag) { throw "no core tag pinned in Cargo.toml" }
    $corePin = ([regex]'(?ms)name = "silentsilo-core".*?source = "git\+[^"#]+#([0-9a-f]{40})"').Match(
        (Get-Content Cargo.lock -Raw)).Groups[1].Value
    if (-not $corePin) { throw "Cargo.lock records no commit for silentsilo-core" }
    Write-Host "  core $coreTag ($($corePin.Substring(0,7)))"

    $coreDir = Join-Path ([System.IO.Path]::GetTempPath()) "silentsilo-core-$coreTag"
    if (Test-Path $coreDir) { Remove-Item $coreDir -Recurse -Force }
    # Long paths: core's fixtures name Touch ID keys by their full id.
    git clone --quiet -c core.longpaths=true --branch $coreTag --depth 1 https://github.com/silentsilo/core $coreDir
    if (-not $?) { throw "cloning core $coreTag failed" }

    # A tag can be moved after the fact; the lockfile's commit cannot. If the
    # two disagree, the app was compiled against something other than what is
    # about to be built here.
    $cloned = (git -C $coreDir rev-parse HEAD).Trim()
    if ($cloned -ne $corePin) {
        throw "core $coreTag is at $cloned, but Cargo.lock pins $corePin"
    }

    Push-Location $coreDir
    try {
        cargo build -p silentsilo-extract --release --locked
        if (-not $?) { throw "windows extractor build failed" }
        Copy-Item (Join-Path $coreDir "target\release\silentsilo-extract.exe") `
            (Join-Path $out "silentsilo-extract-windows-x86_64.exe") -Force
    }
    finally {
        Pop-Location
    }

    # Absolute: .NET reads relative paths against the process's directory,
    # which Set-Location does not move.
    $shipped = @((Join-Path $repoRoot "target\release\silentsilo.exe"),
        (Join-Path $out "silentsilo-extract-windows-x86_64.exe"))
    if ($shipHost) { $shipped += (Join-Path $repoRoot $sidecar) }
    Assert-NoVcRuntime $shipped

    # Everything is compiled. Only now the token: it signs the app as Tauri
    # bundles it, the browser host beside it, the installer and the
    # extractor, and nothing that ran a build script.
    Write-Host "`n== Authenticode ==" -ForegroundColor Cyan
    Read-Host "Plug in the signing token and press Enter" | Out-Null
    npx --no-install tauri bundle @tauriConfigs
    if (-not $?) { throw "tauri bundle failed" }

    # The workspace root is the repository root, so cargo writes to .\target,
    # not to src-tauri\target. Getting this wrong looks like a failed build
    # when the build actually succeeded.
    $nsisDir = "target\release\bundle\nsis"
    # Matched on the version, not on "most recent": installers from earlier
    # tags stay in this directory, and shipping one of those under this tag's
    # release notes is the kind of mistake nobody catches until an update
    # misbehaves.
    $setup = Get-ChildItem $nsisDir -Filter "*_${version}_*-setup.exe" |
        Select-Object -First 1
    if (-not $setup) { throw "no installer for $version in $nsisDir" }
    # Copied, then signed where it ships. A .sig left in the bundle folder by
    # an earlier build is never picked up.
    Copy-Item $setup.FullName (Join-Path $out $setup.Name) -Force

    # Tauri bundles the app, not this, so its Authenticode signature is applied
    # by hand. Here rather than next to the minisign signing below, because
    # that one covers the bytes as they ship and Authenticode changes them.
    & "$PSScriptRoot\sign-windows.ps1" -Path (Join-Path $out "silentsilo-extract-windows-x86_64.exe")
    if (-not $?) { throw "signing the windows extractor failed" }

    Write-Host "`nThe token can come out now." -ForegroundColor Gray

    # What the release workflow and the Mac attached to the draft, in a
    # folder of its own: only their signatures go up from here.
    Write-Host "`n== From the draft ==" -ForegroundColor Cyan
    $draft = Join-Path ([System.IO.Path]::GetTempPath()) "silentsilo-draft-$tag"
    if (Test-Path $draft) { Remove-Item $draft -Recurse -Force }
    New-Item -ItemType Directory -Path $draft | Out-Null
    $assets = (gh release view $tag --repo $slug --json assets | ConvertFrom-Json).assets.name |
        Where-Object { $_ -notlike "*.sig" -and $_ -ne "latest.json" -and $_ -ne $setup.Name -and
            $_ -ne "silentsilo-extract-windows-x86_64.exe" }
    foreach ($asset in $assets) {
        gh release download $tag --repo $slug --pattern $asset --dir $draft --clobber
        if (-not $?) { throw "downloading $asset from the draft failed" }
    }

    # A file the release workflow built carries a provenance statement for
    # this tag and this workflow. Anything else on the draft from CI is
    # refused: it is exactly what someone with write access to the
    # repository, and no key, could put there. The macOS app and extractor
    # come from the Mac and have none; Gatekeeper checks their Developer ID
    # signature.
    $identity = "https://github.com/$slug/.github/workflows/release.yml@refs/tags/$tag"
    foreach ($file in Get-ChildItem $draft -File) {
        if ($file.Name -like "*.dmg" -or $file.Name -like "*.app.tar.gz" -or
            $file.Name -eq "silentsilo-extract-macos-universal") {
            Write-Host ("  {0,-46} from the Mac" -f $file.Name)
            continue
        }
        gh attestation verify $file.FullName --repo $slug --cert-identity $identity | Out-Null
        if (-not $?) { throw "$($file.Name) has no provenance from this tag's release workflow" }
        Write-Host ("  {0,-46} built by the release workflow" -f $file.Name)
    }

    # The updater signature on every file: what the updater installs, and
    # the extractors and the rest, so "is this what the project published"
    # can be answered without trusting GitHub. One password for all of them.
    Write-Host "`n== Updater signatures ==" -ForegroundColor Cyan
    if ((Get-FileHash $signer -Algorithm SHA256).Hash -ne $signerHash) {
        throw "the updater signer changed on disk since it was built"
    }
    $toSign = @((Join-Path $out $setup.Name), (Join-Path $out "silentsilo-extract-windows-x86_64.exe")) +
        @(Get-ChildItem $draft -File | ForEach-Object FullName)
    & $signer --key $keyFile --pubkey $pubkey --version $version @toSign
    if ($LASTEXITCODE -ne 0) { throw "the updater signatures failed" }
    foreach ($file in Get-ChildItem $draft -File -Filter *.sig) {
        Copy-Item $file.FullName (Join-Path $out $file.Name) -Force
    }
    foreach ($path in $toSign) {
        $sig = Join-Path $out ("{0}.sig" -f (Split-Path $path -Leaf))
        if (-not (Test-Path $sig)) { throw "$sig was not written" }
    }

    # latest.json is what the updater actually reads. tauri-action writes it
    # in CI; built by hand it has to match byte for byte in structure, and the
    # URL has to point at the asset name uploaded to the release.
    Write-Host "`n== latest.json ==" -ForegroundColor Cyan

    # The platform map lives in release-manifest.ps1, so it can be exercised
    # without the signing token. See the comments there for why each key is
    # shaped the way it is.
    . (Join-Path $PSScriptRoot "release-manifest.ps1")
    $platforms = New-ManifestPlatforms -Tag $tag -Slug $slug -OutDir $out -SetupName $setup.Name

    $manifest = [ordered]@{
        version   = $version
        notes     = "See the release notes for $tag."
        pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
        platforms = $platforms
    }
    # Written through .NET, not Out-File: PowerShell 5.1's utf8 puts a BOM in
    # front, the worker parses this file as JSON straight out of KV, and a
    # parser that refuses the BOM turns every update check into a 500 with
    # nothing pointing back here.
    $json = $manifest | ConvertTo-Json -Depth 5
    [System.IO.File]::WriteAllText(
        (Join-Path $out "latest.json"), $json,
        (New-Object System.Text.UTF8Encoding $false))

    # Read back what was written, not what was meant. Every installed app
    # reads this file, and each check here is a way a release has of failing
    # quietly: wrong version and the updater loops or skips, empty signature
    # and it refuses the download, wrong URL and it 404s.
    $check = Get-Content (Join-Path $out "latest.json") -Raw | ConvertFrom-Json
    if ($check.version -ne $version) {
        throw "latest.json carries $($check.version), the build is $version"
    }
    $win = $check.platforms."windows-x86_64"
    if (-not $win.signature) { throw "latest.json has no updater signature" }
    if ($win.url -notlike "*/download/$tag/$($setup.Name)") {
        throw "latest.json points at the wrong asset: $($win.url)"
    }
    # Every entry, not just Windows: an empty signature makes the app refuse
    # the download it was just offered, and a URL under another tag 404s.
    foreach ($key in $check.platforms.PSObject.Properties.Name) {
        $entry = $check.platforms.$key
        if (-not $entry.signature) { throw "latest.json: $key has no signature" }
        if ($entry.url -notlike "*/download/$tag/*") {
            throw "latest.json: $key points outside $tag at $($entry.url)"
        }
    }
    Write-Host ("  platforms: {0}" -f (($check.platforms.PSObject.Properties.Name) -join ", "))

    # Semver ranks a prerelease below its release, so an rc shipped as plain
    # x.y.z makes the next rc look like a downgrade. Ask the live endpoint
    # what it serves and refuse a version the updater would not offer.
    $live = $null
    try {
        $live = (Invoke-RestMethod "https://releases.silentsilo.com/windows/x86_64/0.0.0-probe" -TimeoutSec 10).version
    } catch {}
    if ($live) {
        if (-not (Test-VersionAbove -Version $version -Than $live)) {
            throw "the endpoint already serves $live, and $version does not rank above it"
        }
    }
    else {
        Write-Host "The update endpoint served no manifest; skipped the version-order check." -ForegroundColor Yellow
    }

    # An unsigned installer that looks finished is the failure this catches:
    # signCommand not reached, token not plugged in, middleware not running.
    # Each of those produces a complete release that warns every person who
    # downloads it, and none of them announces itself.
    Write-Host "`n== Signatures ==" -ForegroundColor Cyan
    # The browser host is checked where Tauri signed it, before it was packed:
    # it ships inside the installer, not beside it.
    $signed = @((Join-Path $out $setup.Name), (Join-Path $out "silentsilo-extract-windows-x86_64.exe"))
    if ($shipHost) { $signed += Join-Path $repoRoot $sidecar }
    foreach ($path in $signed) {
        $name = Split-Path $path -Leaf
        if (-not (Test-Path $path)) { throw "$name is missing" }
        $signature = Get-AuthenticodeSignature $path
        if ($signature.Status -ne "Valid") {
            throw "$name is not validly signed: $($signature.Status). $($signature.StatusMessage)"
        }
        Write-Host ("  {0,-46} {1}" -f $name, $signature.SignerCertificate.Subject)
    }

    Write-Host "`nReady to upload from dist-release\:" -ForegroundColor Green
    Get-ChildItem $out | ForEach-Object {
        "{0,-52} {1,10:N0} bytes" -f $_.Name, $_.Length
    }
    Write-Host "`n  gh release upload $tag (Get-ChildItem dist-release\ | % FullName)" -ForegroundColor Gray
}
finally {
    Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
    Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY_PASSWORD -ErrorAction SilentlyContinue
}
