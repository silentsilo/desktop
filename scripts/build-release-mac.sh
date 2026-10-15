#!/bin/bash
# Builds, signs and notarises the macOS release on the Mac: the universal
# .app inside a .dmg, the .app.tar.gz the updater downloads, and the
# universal extractor (silentsilo-extract-macos-universal). The
# Developer ID certificate and the notarisation password stay in this Mac's
# keychain; nothing of them is on GitHub.
#
# Run it in Terminal on the Mac, from the repo root, at the release tag:
#
#   ./scripts/build-release-mac.sh
#
# Terminal, not an SSH session: SSH cannot reach the login keychain, and
# codesign then fails on a key it cannot use.
#
# Once, beforehand:
#   - the Developer ID Application certificate in the login keychain;
#   - the notarisation password stored as a keychain profile:
#       xcrun notarytool store-credentials silentsilo-notary \
#         --apple-id <Apple ID> --team-id J3YBNT5RQ3
#   - Google's desktop client secret, set with
#     launchctl setenv SILENTSILO_GOOGLE_CLIENT_SECRET <secret>
#     or kept in ~/.silentsilo-release/google-client-secret.txt
#
# The updater signature is not made here: the release machine signs every
# platform's files with the updater key, after this script has uploaded
# them to the draft (scripts\build-release-local.ps1).
#
# Everything lands in dist-release-mac/.

set -euo pipefail

cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"

# The Developer ID Application certificate of SOFTWARE HIVE S.R.L.
# (J3YBNT5RQ3), by its SHA-1, since the name alone can match two.
IDENTITY="${SILENTSILO_MAC_IDENTITY:-0C1986B4808319E4ABB620C8C56510FAD12B5554}"
PROFILE="${SILENTSILO_NOTARY_PROFILE:-silentsilo-notary}"
ENTITLEMENTS="src-tauri/entitlements.plist"
OUT="dist-release-mac"

version=$(node -p 'require("./package.json").version')
tag="v$version"
echo "Building SilentSilo $version for macOS"

# The three version-bearing files must agree: the updater compares one, the
# binary reports another.
conf=$(node -p 'require("./src-tauri/tauri.conf.json").version')
cargo_version=$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -1)
workspace_version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
[ "$conf" = "$version" ] || { echo "tauri.conf.json says $conf, package.json $version"; exit 1; }
[ "$cargo_version" = "$version" ] || { echo "src-tauri/Cargo.toml says $cargo_version, package.json $version"; exit 1; }
[ "$workspace_version" = "$version" ] || { echo "Cargo.toml (workspace) says $workspace_version, package.json $version"; exit 1; }

# Built from the tag, against the core the lockfile pins, or not at all.
[ ! -e .cargo/config.toml ] || { echo ".cargo/config.toml patches core to a local checkout. Remove it and run cargo check first."; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "The working tree is not clean."; exit 1; }
git tag --points-at HEAD | grep -qx "$tag" || { echo "HEAD is not tagged $tag."; exit 1; }
node scripts/check-lockfile.mjs

# No signing key belongs on GitHub: anyone who can push a tag and a changed
# workflow can read a secret there. Checked by name, on the repository and
# each of its environments, before anything is built or signed.
command -v gh > /dev/null || { echo "gh is needed (brew install gh, then gh auth login)."; exit 1; }
gh auth status > /dev/null 2>&1 || { echo "gh is not signed in (gh auth login)."; exit 1; }
# The release workflow opens the draft this uploads to; a published release
# is never written to, since attaching would put it back in draft.
draft=$(gh release view "$tag" --repo silentsilo/desktop --json isDraft -q .isDraft 2>/dev/null || echo absent)
[ "$draft" = "true" ] || { echo "No draft release $tag (it is $draft). Push the tag and wait for the release workflow first."; exit 1; }
repo=silentsilo/desktop
pattern='^(TAURI_SIGNING_|APPLE_)|SIGN|CERTIFICATE|PRIVATE_KEY|KEYSTORE|(^|_)P12($|_)'
found=$(gh secret list --repo "$repo" --json name --jq '.[].name' | grep -E "$pattern" | sed 's/$/ (repository)/' || true)
for env in $(gh api "repos/$repo/environments" --jq '.environments[].name'); do
  found="$found$(gh secret list --repo "$repo" --env "$env" --json name --jq '.[].name' \
    | grep -E "$pattern" | sed "s/\$/ (environment $env)/" || true)"
done
if [ -n "$found" ]; then
  echo "Signing secrets are on GitHub, where a pushed tag can read them:"
  echo "$found"
  echo "Delete them, then build."
  exit 1
fi

secret_file="$HOME/.silentsilo-release/google-client-secret.txt"
if [ -z "${SILENTSILO_GOOGLE_CLIENT_SECRET:-}" ]; then
  SILENTSILO_GOOGLE_CLIENT_SECRET=$(launchctl getenv SILENTSILO_GOOGLE_CLIENT_SECRET || true)
fi
if [ -z "${SILENTSILO_GOOGLE_CLIENT_SECRET:-}" ] && [ -f "$secret_file" ]; then
  SILENTSILO_GOOGLE_CLIENT_SECRET=$(tr -d '[:space:]' < "$secret_file")
fi
[ -n "${SILENTSILO_GOOGLE_CLIENT_SECRET:-}" ]   || { echo "No Google client secret (launchctl or $secret_file). Without it this build has no Google Drive."; exit 1; }
export SILENTSILO_GOOGLE_CLIENT_SECRET

# Asked before the long build, so a missing certificate does not show up
# an hour in.
security find-identity -v -p codesigning | grep -qF "$IDENTITY" \
  || { echo "No codesigning identity \"$IDENTITY\" in the keychain."; exit 1; }
xcrun notarytool history --keychain-profile "$PROFILE" > /dev/null \
  || { echo "The notarisation profile $PROFILE does not work."; exit 1; }

rm -rf "$OUT"
mkdir -p "$OUT"
rustup target add aarch64-apple-darwin x86_64-apple-darwin > /dev/null

echo "== Extractor"
# From a clean clone of the core tag this app pins, at the commit the
# lockfile records: a tag can move, the lockfile's commit cannot. Built
# before the app, so a problem here shows up in minutes. Both architectures
# in one binary, signed and notarised, so it starts on any Mac from a
# browser download. A bare binary cannot be stapled; Gatekeeper finds the
# ticket online.
core_tag=$(grep -m1 -oE 'silentsilo-core = \{ git = "[^"]+", tag = "([^"]+)"' Cargo.toml \
  | grep -oE 'tag = "[^"]+"' | cut -d'"' -f2)
core_commit=$(awk '/^name = "silentsilo-core"$/{f=1} f && /^source = /{print; exit}' Cargo.lock \
  | grep -oE '#[0-9a-f]{40}' | tr -d '#')
[ -n "$core_tag" ] && [ -n "$core_commit" ] \
  || { echo "Cargo.toml pins no core tag, or Cargo.lock records no commit for it."; exit 1; }
extract_work=$(mktemp -d)
core_src="$extract_work/core"
git clone --quiet --branch "$core_tag" --depth 1 https://github.com/silentsilo/core "$core_src"
got=$(git -C "$core_src" rev-parse HEAD)
[ "$got" = "$core_commit" ] || { echo "core $core_tag is at $got, but Cargo.lock pins $core_commit."; exit 1; }
extractor="$OUT/silentsilo-extract-macos-universal"
(
  cd "$core_src"
  cargo test -p silentsilo-extract --release --locked
  for arch in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build -p silentsilo-extract --release --locked --target "$arch"
  done
)
lipo -create -output "$extractor" \
  "$core_src/target/aarch64-apple-darwin/release/silentsilo-extract" \
  "$core_src/target/x86_64-apple-darwin/release/silentsilo-extract"
lipo "$extractor" -verify_arch arm64 x86_64
codesign --force --options runtime --timestamp --identifier com.silentsilo.extract \
  --sign "$IDENTITY" "$extractor"
codesign --verify --strict --verbose=2 "$extractor"
ditto -c -k "$extractor" "$extract_work/extract.zip"
# Read whole before matching: under pipefail, grep -q closing the pipe early
# would fail the submit and read as a refusal.
notarised=$(xcrun notarytool submit "$extract_work/extract.zip" --keychain-profile "$PROFILE" \
  --wait --output-format json)
echo "$notarised"
echo "$notarised" | grep -Eq '"status" *: *"Accepted"' || { echo "The extractor was not notarised."; exit 1; }
rm -rf "$extract_work"

echo "== Build"
npm ci

# The browser extension's host, shipped once a store lists the extension:
# built for both architectures, joined into one binary for the universal
# app, and checked with --check-release. The bundler puts it in
# Contents/MacOS beside the app, and it is signed below with the rest.
# (The odd expansion below is for the bash 3.2 macOS ships, where an empty
# array under `set -u` is an error.)
host_config=()
listed=$(node -p 'const o=require("./crates/silentsilo-browser-host/allowed-origins.json"); ["chrome_web_store","edge_add_ons","firefox_add_ons"].reduce((n,k)=>n+(o[k]||[]).length,0)')
if [ "$listed" -gt 0 ]; then
  echo "== Browser host"
  for arch in aarch64-apple-darwin x86_64-apple-darwin; do
    cargo build -p silentsilo-browser-host --release --locked --target "$arch"
  done
  mkdir -p src-tauri/binaries
  # A universal build compiles each architecture on its own first, and each
  # looks for its own copy.
  for arch in aarch64-apple-darwin x86_64-apple-darwin; do
    cp "target/$arch/release/silentsilo-browser-host" "src-tauri/binaries/silentsilo-browser-host-$arch"
  done
  lipo -create -output src-tauri/binaries/silentsilo-browser-host-universal-apple-darwin \
    target/aarch64-apple-darwin/release/silentsilo-browser-host \
    target/x86_64-apple-darwin/release/silentsilo-browser-host
  src-tauri/binaries/silentsilo-browser-host-universal-apple-darwin --check-release
  host_config=(--config src-tauri/tauri.browser-host.json)
else
  echo "No store lists the extension: this release ships without the browser host."
fi

# The .app only, unsigned: signing comes after, with the options below,
# and the .dmg is made around the signed and notarised app.
npx tauri build --target universal-apple-darwin --bundles app \
  --config '{"bundle":{"createUpdaterArtifacts":false}}' ${host_config[@]+"${host_config[@]}"} -- --locked
if [ "$listed" -gt 0 ] && [ ! -x "target/universal-apple-darwin/release/bundle/macos/SilentSilo.app/Contents/MacOS/silentsilo-browser-host" ]; then
  echo "The browser host is not inside the app."; exit 1
fi

bundle=target/universal-apple-darwin/release/bundle/macos
app=$(cd "$bundle" && ls -d *.app)
name="${app%.app}"
work=$(mktemp -d)
cp -R "$bundle/$app" "$work/"
signed="$work/$app"
[ -f "$signed/Contents/embedded.provisionprofile" ] || { echo "No provisioning profile inside the app."; exit 1; }

echo "== Sign"
# Inside out: the helpers beside the main binary, any framework, then the
# app with its entitlements, hardened runtime and a secure timestamp.
main=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$signed/Contents/Info.plist")
for helper in "$signed/Contents/MacOS/"*; do
  [ "$(basename "$helper")" = "$main" ] && continue
  codesign --force --options runtime --timestamp --sign "$IDENTITY" "$helper"
done
if [ -d "$signed/Contents/Frameworks" ]; then
  find "$signed/Contents/Frameworks" -mindepth 1 -maxdepth 1 \
    -exec codesign --force --options runtime --timestamp --sign "$IDENTITY" {} \;
fi
codesign --force --options runtime --timestamp --entitlements "$ENTITLEMENTS" \
  --sign "$IDENTITY" "$signed"
codesign --verify --strict --deep --verbose=2 "$signed"

echo "== Notarise the app"
ditto -c -k --keepParent "$signed" "$work/app.zip"
xcrun notarytool submit "$work/app.zip" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$signed"

echo "== Disk image"
# The stapled app with the Applications link people drag it onto, then
# signed, notarised and stapled itself. Named the way the bundler names it.
mkdir "$work/dmg"
cp -R "$signed" "$work/dmg/"
ln -s /Applications "$work/dmg/Applications"
dmg="$OUT/${name}_${version}_universal.dmg"
hdiutil create -volname "$name" -srcfolder "$work/dmg" -ov -format UDZO "$dmg"
codesign --force --timestamp --sign "$IDENTITY" "$dmg"
xcrun notarytool submit "$dmg" --keychain-profile "$PROFILE" --wait
xcrun stapler staple "$dmg"

echo "== Updater archive"
# After notarisation, so the ticket travels inside. COPYFILE_DISABLE keeps
# AppleDouble ._ files out of it.
COPYFILE_DISABLE=1 tar -C "$work" -czf "$OUT/$app.tar.gz" "$app"

echo "== Check"
spctl --assess --type exec --verbose=4 "$signed"
xcrun stapler validate "$signed"
xcrun stapler validate "$dmg"
rm -rf "$work"

ls -l "$OUT"

echo "== Upload to the draft"
gh release upload "$tag" "$OUT"/* --repo silentsilo/desktop --clobber
cat <<EOF

Uploaded to the draft $tag. Next, on the release machine:
scripts\\build-release-local.ps1 signs them for the updater with the rest.
EOF
