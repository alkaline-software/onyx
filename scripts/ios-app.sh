#!/bin/sh
# Put the simulator's build in an iPhone or iPad simulator and start it
# there, as dev-app.sh starts the Mac's build.
#
#   cargo build -p onyx-app --target aarch64-apple-ios-sim
#   scripts/ios-app.sh [--stay] [--log <file>] [a simulator's name or id]
#
# With no simulator named it is the one already booted, else the iPad
# below. --stay installs the app and does not start it. --log writes what
# the app prints to that file, for as long as the app runs.
#
# An app on an iPhone is a bundle with an Info.plist and a compiled asset
# catalog (the icon, the launch screen's color), and no packager makes one
# from a Cargo manifest, so it is made here: the executable, ios/Info.plist
# with the version and the dev build's own identifier in it, and
# ios/Assets.xcassets through actool, which also says which icon files the
# bundle has. A simulator takes a bundle signed by nobody.
#
# ONYX_* go through to the app (docs/probing.md), as SIMCTL_CHILD_ONYX_*,
# which is how simctl hands a variable to what it launches.

cd "$(dirname "$0")/.." || exit 1
[ "$(uname)" = Darwin ] || { echo "ios-app: a simulator runs on a Mac" >&2; exit 1; }
stay=""
log=""
while [ $# -gt 0 ]; do
    case "$1" in
        --stay) stay=1; shift ;;
        --log) log="$2"; shift 2 ;;
        *) break ;;
    esac
done

[ "$(uname -m)" = arm64 ] && triple=aarch64-apple-ios-sim || triple=x86_64-apple-ios
bin="target/$triple/debug/Onyx"
[ -x "$bin" ] || { echo "no $bin: cargo build -p onyx-app --target $triple" >&2; exit 1; }

id=com.alkalinesoftware.onyx.dev
app="target/$triple/debug/ios.noindex/Onyx.app"
rm -rf "$app"
mkdir -p "$app"
cp -c "$bin" "$app/Onyx" 2>/dev/null || cp "$bin" "$app/Onyx"
cp crates/onyx-app/ios/Info.plist "$app/Info.plist"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
least="$(sed -n 's/^IPHONEOS_DEPLOYMENT_TARGET = "\(.*\)"/\1/p' .cargo/config.toml)"
plist() { /usr/libexec/PlistBuddy -c "$1" "$app/Info.plist" >/dev/null || exit 1; }
plist "Set :CFBundleIdentifier $id"
plist "Set :CFBundleShortVersionString $version"
plist "Set :MinimumOSVersion $least"
plist "Add :CFBundleSupportedPlatforms array"
plist "Add :CFBundleSupportedPlatforms:0 string iPhoneSimulator"

# The asset catalog compiled into the bundle; what actool says of the icon
# (CFBundleIcons, for an iPhone and for an iPad) goes into the Info.plist.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
xcrun actool crates/onyx-app/ios/Assets.xcassets --compile "$app" \
    --platform iphonesimulator --minimum-deployment-target "$least" \
    --target-device iphone --target-device ipad --app-icon AppIcon \
    --output-partial-info-plist "$tmp/icons.plist" >"$tmp/actool.log" 2>&1 || { cat "$tmp/actool.log" >&2; exit 1; }
plist "Merge $tmp/icons.plist"
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
