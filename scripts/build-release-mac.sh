#!/bin/bash
# Builds, signs and notarises the macOS release on the Mac: the universal
# .app inside a .dmg, and the .app.tar.gz the updater downloads. The
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
[ "$conf" = "$version" ] || { echo "tauri.conf.json says $conf, package.json $version"; exit 1; }
[ "$cargo_version" = "$version" ] || { echo "Cargo.toml says $cargo_version, package.json $version"; exit 1; }

# Built from the tag, against the core the lockfile pins, or not at all.
[ ! -e .cargo/config.toml ] || { echo ".cargo/config.toml patches core to a local checkout. Remove it and run cargo check first."; exit 1; }
[ -z "$(git status --porcelain)" ] || { echo "The working tree is not clean."; exit 1; }
git tag --points-at HEAD | grep -qx "$tag" || { echo "HEAD is not tagged $tag."; exit 1; }
node scripts/check-lockfile.mjs

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

echo "== Build"
npm ci
rustup target add aarch64-apple-darwin x86_64-apple-darwin > /dev/null
# The .app only, unsigned: signing comes after, with the options below,
# and the .dmg is made around the signed and notarised app.
npx tauri build --target universal-apple-darwin --bundles app \
  --config '{"bundle":{"createUpdaterArtifacts":false}}' -- --locked

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
cat <<EOF

Ready. Attach these to the draft, then sign them for the updater on the
release machine with scripts\\build-release-local.ps1:

  gh release upload $tag $OUT/* --repo silentsilo/desktop
EOF
