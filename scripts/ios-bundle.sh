#!/bin/sh
# Make an iPhone's and an iPad's app bundle from a built executable, signed
# by nobody: ios-app.sh signs it for a simulator or a device, and the
# release signs it for the store.
#
#   scripts/ios-bundle.sh <executable> <iphonesimulator|iphoneos> <identifier> <out.app>
#
# An app on an iPhone is a bundle with an Info.plist and a compiled asset
# catalog (the icon, the launch screen's color), and no packager makes one
# from a Cargo manifest, so it is made here: the executable, ios/Info.plist
# with the version, the identifier and the platform in it, the privacy
# manifest, and ios/Assets.xcassets through actool, which also says which
# icon files the bundle has. ONYX_BUILD is the build's number
# (CFBundleVersion), which the store wants higher at every upload of a
# version; 0 without it.

cd "$(dirname "$0")/.." || exit 1
[ $# -eq 4 ] || { echo "ios-bundle: <executable> <iphonesimulator|iphoneos> <identifier> <out.app>" >&2; exit 2; }
bin="$1"; platform="$2"; id="$3"; app="$4"
[ -x "$bin" ] || { echo "ios-bundle: no $bin" >&2; exit 1; }
case "$platform" in
    iphonesimulator) supported=iPhoneSimulator ;;
    iphoneos) supported=iPhoneOS ;;
    *) echo "ios-bundle: the platform is iphonesimulator or iphoneos" >&2; exit 2 ;;
esac

rm -rf "$app"
mkdir -p "$app"
cp -c "$bin" "$app/Onyx" 2>/dev/null || cp "$bin" "$app/Onyx"
cp crates/onyx-app/ios/Info.plist "$app/Info.plist"
cp crates/onyx-app/ios/PrivacyInfo.xcprivacy "$app/PrivacyInfo.xcprivacy"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
least="$(sed -n 's/^IPHONEOS_DEPLOYMENT_TARGET = "\(.*\)"/\1/p' .cargo/config.toml)"
plist() { /usr/libexec/PlistBuddy -c "$1" "$app/Info.plist" >/dev/null || exit 1; }
plist "Set :CFBundleIdentifier $id"
plist "Set :CFBundleShortVersionString $version"
plist "Set :CFBundleVersion ${ONYX_BUILD:-0}"
plist "Set :MinimumOSVersion $least"
plist "Add :CFBundleSupportedPlatforms array"
plist "Add :CFBundleSupportedPlatforms:0 string $supported"

# What Xcode writes of itself into a device's bundle, which the store
# reads: the SDK and the tools the app was built with.
if [ "$platform" = iphoneos ]; then
    sdk="$(xcrun --sdk iphoneos --show-sdk-version)"
    sdk_build="$(xcrun --sdk iphoneos --show-sdk-build-version)"
    xcode="$(xcodebuild -version | sed -n 's/^Xcode \([0-9]*\)\.\([0-9]*\).*/\1 \2/p')"
    xcode_build="$(xcodebuild -version | sed -n 's/^Build version //p')"
    # shellcheck disable=SC2086 # the major and the minor are two words on purpose
    plist "Add :DTXcode string $(printf '%02d%d0' $xcode)"
    plist "Add :DTXcodeBuild string $xcode_build"
    plist "Add :DTCompiler string com.apple.compilers.llvm.clang.1_0"
    plist "Add :DTPlatformName string iphoneos"
    plist "Add :DTPlatformVersion string $sdk"
    plist "Add :DTPlatformBuild string $sdk_build"
    plist "Add :DTSDKName string iphoneos$sdk"
    plist "Add :DTSDKBuild string $sdk_build"
    plist "Add :BuildMachineOSBuild string $(sw_vers -buildVersion)"
    plist "Add :UIRequiredDeviceCapabilities array"
    plist "Add :UIRequiredDeviceCapabilities:0 string arm64"
fi

# The asset catalog compiled into the bundle; what actool says of the icon
# (CFBundleIcons, for an iPhone and for an iPad) goes into the Info.plist.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
xcrun actool crates/onyx-app/ios/Assets.xcassets --compile "$app" \
    --platform "$platform" --minimum-deployment-target "$least" \
    --target-device iphone --target-device ipad --app-icon AppIcon \
    --output-partial-info-plist "$tmp/icons.plist" >"$tmp/actool.log" 2>&1 || { cat "$tmp/actool.log" >&2; exit 1; }
plist "Merge $tmp/icons.plist"
