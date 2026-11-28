#!/bin/zsh
# The iPhone's and iPad's release: the device's build in a bundle
# (ios-bundle.sh), signed with the Apple Distribution certificate, the
# entitlements and the App Store profile, and zipped as an .ipa. With
# --validate App Store Connect checks it and keeps nothing; with --upload
# it is checked and sent, and shows there under TestFlight, where it can
# be tried and chosen for a version of the store's page.
#
#   scripts/release-ios.sh [--no-build] [--validate | --upload] [--out <path.ipa>]
#
# The profile is ONYX_IOS_STORE_PROFILE, else the one docs/release.md has
# put beside the Mac's. The identity is $SIGN_IDENTITY, else the first
# "Apple Distribution" identity in the keychain. App Store Connect is
# reached with an API key: ASC_KEY_ID and ASC_ISSUER_ID, the key's file
# where altool looks for it (~/.appstoreconnect/private_keys/AuthKey_<id>.p8).
#
# The build's number (CFBundleVersion) is ONYX_BUILD, else the count of
# commits, which grows with every one: the store takes a number once.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

BUILD=1; SEND=""; OUT=dist/Onyx-ios.ipa
while [[ $# -gt 0 ]]; do
  case "$1" in
    --no-build) BUILD=0; shift ;;
    --validate) SEND=validate; shift ;;
    --upload) SEND=upload; shift ;;
    --out) OUT="$2"; shift 2 ;;
    *) echo "unknown option $1" >&2; exit 2 ;;
  esac
done
fail() { print -u2 "release-ios: $1"; exit 1; }

PROFILE="${ONYX_IOS_STORE_PROFILE:-$HOME/Library/MobileDevice/Provisioning Profiles/Onyx_App_Store.mobileprovision}"
[[ -f "$PROFILE" ]] || fail "no App Store profile at $PROFILE (docs/release.md, One-time setup)"
IDENTITY="${SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Distribution: [^"]*\)".*/\1/p' | head -1)}"
[[ -n "$IDENTITY" ]] || fail "no Apple Distribution identity in the keychain"
if [[ -n "$SEND" ]]; then
  [[ -n "${ASC_KEY_ID:-}" && -n "${ASC_ISSUER_ID:-}" ]] || fail "ASC_KEY_ID and ASC_ISSUER_ID are not set (docs/release.md, One-time setup)"
  [[ -f "$HOME/.appstoreconnect/private_keys/AuthKey_$ASC_KEY_ID.p8" ]] || fail "no key file ~/.appstoreconnect/private_keys/AuthKey_$ASC_KEY_ID.p8"
fi

TARGET=aarch64-apple-ios
if (( BUILD )); then
  cargo build --release --locked -p onyx-app --target "$TARGET"
fi
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
export ONYX_BUILD="${ONYX_BUILD:-$(git rev-list --count HEAD)}"
STAGE=dist/ios-staging
APP="$STAGE/Payload/Onyx.app"
rm -rf "$STAGE" "$OUT"
mkdir -p "$STAGE/Payload" "$(dirname "$OUT")"
scripts/ios-bundle.sh "target/$TARGET/release/Onyx" iphoneos com.alkalinesoftware.onyx "$APP"

# The store's signature: the entitlements with the team's ID, read from
# the profile, in place of TEAM_ID, and the two keys Xcode adds to a
# bundle for the store: TestFlight's, and iCloud's production side.
security cms -D -i "$PROFILE" > "$STAGE/profile.plist" 2>/dev/null || fail "$PROFILE is not a profile"
TEAM_ID=$(/usr/libexec/PlistBuddy -c 'Print :TeamIdentifier:0' "$STAGE/profile.plist")
cp "$PROFILE" "$APP/embedded.mobileprovision"
sed "s/TEAM_ID/$TEAM_ID/g" crates/onyx-app/ios/Onyx.entitlements > "$STAGE/Onyx.entitlements"
/usr/libexec/PlistBuddy -c 'Add :beta-reports-active bool true' \
  -c 'Add :com.apple.developer.icloud-container-environment string Production' "$STAGE/Onyx.entitlements" > /dev/null
codesign --force --sign "$IDENTITY" --entitlements "$STAGE/Onyx.entitlements" --generate-entitlement-der "$APP"
codesign --verify --strict --verbose=2 "$APP"

# An .ipa is a zip with the bundle in a folder named Payload.
(cd "$STAGE" && zip -qry ipa.zip Payload) && mv "$STAGE/ipa.zip" "$OUT"
rm -rf "$STAGE"
echo "built $OUT ($VERSION, build $ONYX_BUILD, signed for the store)"

auth=(--api-key "${ASC_KEY_ID:-}" --api-issuer "${ASC_ISSUER_ID:-}")
if [[ -n "$SEND" ]]; then
  xcrun altool --validate-app "$OUT" --type ios "${auth[@]}"
fi
if [[ "$SEND" == upload ]]; then
  xcrun altool --upload-package "$OUT" --type ios "${auth[@]}"
  echo "sent $OUT: it shows under TestFlight in App Store Connect once Apple has processed it"
fi
