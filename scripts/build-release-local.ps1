# Builds, signs and names every release artifact on this machine, for the
# case where GitHub Actions cannot run (out of minutes, or the workflow is
# broken). It produces exactly what .github/workflows/release.yml produces,
# under the same file names, so a release assembled by hand is
# indistinguishable from one built by CI.
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
    cargo clippy --all-targets -- -D warnings; if (-not $?) { throw "clippy failed" }
    cargo test --all;  if (-not $?) { throw "cargo test failed" }

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

    # The workspace root is silentsilo.gui, so cargo writes to .\target, not
    # to src-tauri\target. Getting this wrong looks like a failed build when
    # the build actually succeeded.
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

    Write-Host "`n== CLI, Windows ==" -ForegroundColor Cyan
    cargo build -p silentsilo-extract --release
    if (-not $?) { throw "windows extractor build failed" }
    Copy-Item "target\release\silentsilo-extract.exe" `
        (Join-Path $out "silentsilo-extract-windows-x86_64.exe") -Force

    # Tauri bundles the app, not this, so its Authenticode signature is applied
    # by hand. Here rather than next to the minisign signing below, because
    # that one covers the bytes as they ship and Authenticode changes them.
    & "$PSScriptRoot\sign-windows.ps1" -Path (Join-Path $out "silentsilo-extract-windows-x86_64.exe")
    if (-not $?) { throw "signing the windows extractor failed" }

    Write-Host "`n== CLI, Linux ==" -ForegroundColor Cyan
    # Built through WSL against the same source tree. Skipped rather than
    # fatal: a missing Linux binary is worth noticing, not worth throwing away
    # a signed installer over.
    $linuxAsset = Join-Path $out "silentsilo-extract-linux-x86_64"
    $wslRoot = "/mnt/" + $repoRoot.Substring(0, 1).ToLower() + $repoRoot.Substring(2).Replace("\", "/")
    # A separate target directory on purpose: sharing target\ with the Windows
    # build means the two overwrite each other's artifacts and recompile the
    # world on every alternation.
    wsl -d Ubuntu -- bash -lc "cd '$wslRoot' && CARGO_TARGET_DIR=target-linux cargo build -p silentsilo-extract --release"
    if ($?) {
        Copy-Item "target-linux\release\silentsilo-extract" $linuxAsset -Force -ErrorAction SilentlyContinue
    }
    if (-not (Test-Path $linuxAsset)) {
        Write-Host "Linux extractor NOT built. Install the headers once:" -ForegroundColor Yellow
        Write-Host "  wsl -d Ubuntu -- sudo apt-get install -y libdbus-1-dev pkg-config" -ForegroundColor Yellow
    }

    # The extractor is the tool someone reaches for when they no longer trust
    # anything else, so it gets the same signature the installer does. Verify
    # with: tauri signer verify, or minisign -V against the public key.
    Write-Host "`n== Signing the extractors ==" -ForegroundColor Cyan
    # TAURI_SIGNING_PRIVATE_KEY holds a path here, which `tauri build` accepts
    # but `signer sign` would read as the key itself. Clear it so the explicit
    # -f flag is the only source.
    Remove-Item Env:\TAURI_SIGNING_PRIVATE_KEY -ErrorAction SilentlyContinue
    foreach ($name in @("silentsilo-extract-windows-x86_64.exe", "silentsilo-extract-linux-x86_64")) {
        $path = Join-Path $out $name
        if (Test-Path $path) {
            npx --yes @tauri-apps/cli signer sign --private-key-path $keyFile $path
            if (-not $?) { throw "signing $name failed" }
        }
    }

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
