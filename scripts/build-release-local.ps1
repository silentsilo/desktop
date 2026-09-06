# Builds and signs, on this machine, the release artifacts CI cannot: the
# installer, its updater signature, latest.json and the Windows extractor.
# The Authenticode certificate sits on a hardware token no runner reaches,
# which is the whole reason they are built here.
#
# The Linux and macOS extractors come from silentsilo/core, built and signed
# when its tag was pushed. This repository's release workflow downloads them
# from the core release this build is pinned to and attaches them to the same
# draft. They are not rebuilt here on purpose: a binary out of WSL links
# against this machine's glibc, and the extractor is the one tool that has to
# start on a machine nothing is assumed about.
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
# Authenticode needs the hardware token plugged in and unlocked. The updater
# key needs a password, asked for once below; that password never reaches a
# file or the shell history, it is read into the process environment and
# cleared when the script ends.
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

# A `[patch]` pointing Cargo at a local core checkout rewrites Cargo.lock and
# builds the app against whatever sits in a sibling directory rather than
# against the pinned tag. Convenient while developing, catastrophic in a
# signed installer that nobody can trace back to a revision.
if (Test-Path (Join-Path $repoRoot ".cargo\config.toml")) {
    throw ".cargo\config.toml exists. It patches the core crates to a local checkout; remove it, run cargo check to restore Cargo.lock, then build."
}

# Says which core revision this installer will contain, and refuses a lockfile
# that points anywhere but silentsilo/core.
node scripts\check-lockfile.mjs
if (-not $?) { throw "Cargo.lock does not point at silentsilo/core" }

# A release build answers for the tag, so it must be built from the tag: a
# dirty tree or a HEAD the tag does not point at produces an installer whose
# source can never be named again, and nothing downstream detects it.
if (git status --porcelain) {
    throw "Working tree is not clean. Commit or stash, then build."
}
if ((git tag --points-at HEAD) -notcontains $tag) {
    throw "HEAD is not tagged $tag. Tag first (see TUTORIAL-RELEASE.md), then build."
}

$secure = Read-Host "Updater signing key password" -AsSecureString
$env:TAURI_SIGNING_PRIVATE_KEY = $keyFile
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD =
    [System.Runtime.InteropServices.Marshal]::PtrToStringAuto(
        [System.Runtime.InteropServices.Marshal]::SecureStringToBSTR($secure))

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
    # Tauri signs during bundling and computes the updater signature after, so
    # the .sig covers the signed bytes. Getting that order wrong produces an
    # update the app refuses to install, which is why signing does not happen
    # further down with the extractors.
    # Direct, not `npm run tauri:build`: npm drops forwarded arguments for a
    # script chained with `&&`, taking the signing config with them.
    npm run icons; if (-not $?) { throw "icon generation failed" }
    npx tauri build --config src-tauri/tauri.signing.json
    if (-not $?) { throw "tauri build failed" }

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
    $sig = "$($setup.FullName).sig"
    if (-not (Test-Path $sig)) { throw "installer was not signed: $sig missing" }

    Copy-Item $setup.FullName (Join-Path $out $setup.Name) -Force
    Copy-Item $sig (Join-Path $out "$($setup.Name).sig") -Force

    # The extractor lives in silentsilo/core now, so it is built from a clean
    # clone of the tag this app is pinned to, at the exact commit Cargo.lock
    # records. Cloning rather than reusing a sibling working copy: that copy
    # may have uncommitted work, and this binary is the one somebody reaches
    # for when they have lost confidence in everything else.
    #
    # The Linux and macOS extractors come from core's own release workflow.
    # A binary built out of WSL links against this machine's glibc, and the
    # extractor is the one tool that has to start on a machine nothing is
    # assumed about.
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
    git clone --quiet --branch $coreTag --depth 1 https://github.com/silentsilo/core $coreDir
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

    # Tauri bundles the app, not this, so its Authenticode signature is applied
    # by hand. Here rather than next to the minisign signing below, because
    # that one covers the bytes as they ship and Authenticode changes them.
    & "$PSScriptRoot\sign-windows.ps1" -Path (Join-Path $out "silentsilo-extract-windows-x86_64.exe")
    if (-not $?) { throw "signing the windows extractor failed" }

    # The extractor is the tool someone reaches for when they no longer trust
    # anything else, so it gets the same signature the installer does. Verify
    # with minisign against the public key in tauri.conf.json.
    Write-Host "`n== Signing the extractor ==" -ForegroundColor Cyan
    # TAURI_SIGNING_PRIVATE_KEY holds a path here, which `tauri build` accepts
    # but `signer sign` would read as the key itself. Clear it so the explicit
    # -f flag is the only source.
    Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
    npx --yes @tauri-apps/cli signer sign --private-key-path $keyFile `
        (Join-Path $out "silentsilo-extract-windows-x86_64.exe")
    if (-not $?) { throw "the updater signature for the windows extractor failed" }

    # latest.json is what the updater actually reads. tauri-action writes it
    # in CI; built by hand it has to match byte for byte in structure, and the
    # URL has to point at the asset name uploaded to the release.
    Write-Host "`n== latest.json ==" -ForegroundColor Cyan
    $manifest = [ordered]@{
        version   = $version
        notes     = "See the release notes for $tag."
        pub_date  = (Get-Date).ToUniversalTime().ToString("yyyy-MM-ddTHH:mm:ssZ")
        platforms = [ordered]@{
            "windows-x86_64" = [ordered]@{
                signature = (Get-Content (Join-Path $out "$($setup.Name).sig") -Raw).Trim()
                url       = "https://github.com/$slug/releases/download/$tag/$($setup.Name)"
            }
        }
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

    # Semver ranks a prerelease below its release, so an rc shipped as plain
    # x.y.z makes the next rc look like a downgrade. Ask the live endpoint
    # what it serves and refuse a version the updater would not offer.
    $live = $null
    try {
        $live = (Invoke-RestMethod "https://releases.silentsilo.com/windows/x86_64/0.0.0" -TimeoutSec 10).version
    } catch {}
    if ($live) {
        npx --yes semver $version -r "> $live" | Out-Null
        if (-not $?) {
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
    foreach ($name in @($setup.Name, "silentsilo-extract-windows-x86_64.exe")) {
        $path = Join-Path $out $name
        if (-not (Test-Path $path)) { continue }
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
