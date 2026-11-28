#!/bin/sh
# Put the build in an iPhone or iPad simulator, or on a device joined to
# this Mac, and start it there, as dev-app.sh starts the Mac's build.
#
#   cargo build -p onyx-app --target aarch64-apple-ios-sim
#   scripts/ios-app.sh [--stay] [--log <file>] [a simulator's name or id]
#
#   cargo build --release -p onyx-app --target aarch64-apple-ios
#   scripts/ios-app.sh --device [--stay] [--log <file>] [a device's name or id]
#
# With no simulator named it is the one already booted, else the iPad
# below; with no device named, the one joined. --stay installs the app and
# does not start it. --log writes what the app prints to that file, for as
# long as the app runs. --release takes the release build, which a device
# always does: the debug one is too slow in the hand to say anything.
#
# The bundle is ios-bundle.sh's. A simulator takes one signed by nobody,
# under the dev build's own identifier. A device takes one signed with an
# Apple Development certificate, the entitlements (ios/Onyx.entitlements,
# the iCloud container) and a development profile that names the device,
# under Onyx's own identifier, since the profile is for that App ID: it
# stands in the place of the store's copy. The profile is ONYX_IOS_PROFILE,
# else the one docs/release.md has put beside the Mac's.
#
# ONYX_* go through to the app (docs/probing.md): to a simulator as
# SIMCTL_CHILD_ONYX_*, which is how simctl hands a variable to what it
# launches, and to a device in devicectl's own list of them.

cd "$(dirname "$0")/.." || exit 1
[ "$(uname)" = Darwin ] || { echo "ios-app: a simulator and a device are reached from a Mac" >&2; exit 1; }
stay=""
log=""
device=""
profile=debug
while [ $# -gt 0 ]; do
    case "$1" in
        --stay) stay=1; shift ;;
        --log) log="$2"; shift 2 ;;
        --device) device=1; profile=release; shift ;;
        --release) profile=release; shift ;;
        *) break ;;
    esac
done

if [ -n "$device" ]; then
    triple=aarch64-apple-ios
elif [ "$(uname -m)" = arm64 ]; then
    triple=aarch64-apple-ios-sim
else
    triple=x86_64-apple-ios
fi
bin="target/$triple/$profile/Onyx"
flag=""; [ "$profile" = release ] && flag=" --release"
[ -x "$bin" ] || { echo "no $bin: cargo build$flag -p onyx-app --target $triple" >&2; exit 1; }
app="target/$triple/$profile/ios.noindex/Onyx.app"

if [ -n "$device" ]; then
    id=com.alkalinesoftware.onyx
    prov="${ONYX_IOS_PROFILE:-$HOME/Library/MobileDevice/Provisioning Profiles/Onyx_Development.mobileprovision}"
    [ -f "$prov" ] || { echo "ios-app: no development profile at $prov (docs/release.md, One-time setup)" >&2; exit 1; }
    sign="${SIGN_IDENTITY:-$(security find-identity -v -p codesigning 2>/dev/null | sed -n 's/.*"\(Apple Development: [^"]*\)".*/\1/p' | head -1)}"
    [ -n "$sign" ] || { echo "ios-app: no Apple Development identity in the keychain" >&2; exit 1; }
    scripts/ios-bundle.sh "$bin" iphoneos "$id" "$app" || exit 1
    # The team is read from the profile, which is what the device checks
    # the signature against.
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    security cms -D -i "$prov" >"$tmp/profile.plist" 2>/dev/null || { echo "ios-app: $prov is not a profile" >&2; exit 1; }
    team="$(/usr/libexec/PlistBuddy -c 'Print :TeamIdentifier:0' "$tmp/profile.plist")"
    cp "$prov" "$app/embedded.mobileprovision"
    sed "s/TEAM_ID/$team/g" crates/onyx-app/ios/Onyx.entitlements >"$tmp/Onyx.entitlements"
    codesign --force --sign "$sign" --entitlements "$tmp/Onyx.entitlements" --generate-entitlement-der "$app" >"$tmp/sign.log" 2>&1 || { cat "$tmp/sign.log" >&2; exit 1; }

    # The device: the one named, else the one joined.
    dev="${1:-}"
    [ -n "$dev" ] || dev="$(xcrun devicectl list devices 2>/dev/null | awk '$NF == "physical" && /connected|available/ { for (i = 1; i <= NF; i++) if ($i ~ /^[0-9A-F]{8}-[0-9A-F]{16}$/) print $i }' | head -1)"
    [ -n "$dev" ] || { echo "ios-app: no device is joined (xcrun devicectl list devices)" >&2; exit 1; }
    xcrun devicectl device install app --device "$dev" "$app" >"$tmp/install.log" 2>&1 || { cat "$tmp/install.log" >&2; exit 1; }
    echo "Onyx is on $dev"
    [ -n "$stay" ] && exit 0
    vars="$(env | grep -E '^ONYX_[A-Z_]+=' | grep -v ' ' | sed 's/"/\\"/g; s/^\([A-Z_]*\)=\(.*\)$/"\1":"\2"/' | paste -sd, -)"
    set -- xcrun devicectl device process launch --device "$dev" --terminate-existing
    [ -z "$vars" ] || set -- "$@" --environment-variables "{$vars}"
    [ -n "$log" ] || exec "$@" "$id"
    nohup "$@" --console "$id" >"$log" 2>&1 &
    exit 0
fi

id=com.alkalinesoftware.onyx.dev
scripts/ios-bundle.sh "$bin" iphonesimulator "$id" "$app" || exit 1
codesign --force --sign - "$app" >/dev/null 2>&1 || { echo "ios-app: the bundle was not signed" >&2; exit 1; }

# The simulator: the one named, else one already booted, else an iPad.
dev="${1:-}"
[ -n "$dev" ] || dev="$(xcrun simctl list devices booted | sed -n 's/.*(\([0-9A-F-]\{36\}\)) (Booted).*/\1/p' | head -1)"
[ -n "$dev" ] || dev="iPad Pro 11-inch (M5)"
xcrun simctl bootstatus "$dev" -b >/dev/null 2>&1 || { echo "ios-app: no simulator \"$dev\" (xcrun simctl list devices available)" >&2; exit 1; }
xcrun simctl install "$dev" "$app" || exit 1
echo "Onyx is in $dev"
[ -n "$stay" ] && exit 0

# What the app prints comes through the simulator's console, which stays
# with the app until it ends: simctl's own --stdout and --stderr wrote no
# file. So with --log the launch is left running behind this script.
# shellcheck disable=SC2046 # each ONYX_ variable is one word on purpose
set -- env $(env | grep -E '^ONYX_[A-Z_]+=' | grep -v ' ' | sed 's/^/SIMCTL_CHILD_/') xcrun simctl launch --terminate-running-process
[ -n "$log" ] || exec "$@" "$dev" "$id"
nohup "$@" --console-pty "$dev" "$id" >"$log" 2>&1 &
