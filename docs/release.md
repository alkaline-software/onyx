# Releasing Onyx

What to run, in order, to cut a release, and what to do when a step fails.

## What a release is

A `v*` tag pushed to the repository runs `release.yml`. Two package jobs build the app for macOS (both architectures, from the Apple Silicon runner), then one `release` job creates the GitHub Release as a draft, with the tag's section of `CHANGELOG.md` as its notes, and attaches the installers under stable names:

| asset | built by | signed |
|---|---|---|
| `Onyx-mac-arm64.dmg`, `Onyx-mac-x64.dmg` | CI, then replaced from the signer's Mac | Developer ID, notarized, stapled |

The names never change, so `releases/latest/download/<name>` links stay valid.

Only the Mac is published, so the workflow's matrix has only its rows. Its steps for Windows and Linux stay, and a row for each runs them again; with those rows back, the release also carries `Onyx-windows-<arch>-setup.exe`, Velopack's `AlkalineSoftware.Onyx-<version>-win-<arch>-full.nupkg` and `releases.win-<arch>.json`, and `Onyx-linux-<arch>.AppImage` and `.deb`, for x64 and ARM each.

## One-time setup on a Mac

- Rust via rustup, plus `rustup target add x86_64-apple-darwin`.
- `cargo install cargo-packager --version 0.11.8 --locked`, the version `release.yml` pins.
- Xcode or its command line tools (`codesign`, `hdiutil`, `notarytool`, `stapler`).
- A `Developer ID Application` certificate in the login keychain (`security find-identity -v -p codesigning` lists it); `SIGN_IDENTITY` names one when there are several.
- The notarytool keychain profile `notarytool-profile` (`xcrun notarytool store-credentials notarytool-profile` once, with the Apple ID, an app-specific password and the team ID); `NOTARY_PROFILE` names another.
- The iCloud container, once, in the developer account at developer.apple.com: under Identifiers, an App ID for `com.alkalinesoftware.onyx` with the iCloud capability; under Identifiers, iCloud Containers, the container `iCloud.com.alkalinesoftware.onyx`, assigned to that App ID; under Profiles, a Developer ID Application profile for that App ID, downloaded to a folder outside the repository, such as `~/Library/MobileDevice/Provisioning Profiles/Onyx.provisionprofile`, with `ONYX_PROVISION_PROFILE` pointing at it in the shell's profile (in CI, the secret `ONYX_PROVISION_PROFILE_B64`). With it, `release-mac.sh` embeds the profile and signs in `macos/Onyx.entitlements`, and a vault made in iCloud goes in Onyx's app library, the folder Finder shows with the icon on it. Without it, the release runs the same, with iCloud vaults as plain folders under `iCloud Drive/Onyx` (`docs/platform.md`, iCloud Drive).
- `gh auth login` with write access to the repository.
- For the Windows type check: `brew install mingw-w64` and `rustup target add x86_64-pc-windows-gnu`.
- For the disk image's background and the installer's splash: Python with Pillow (`uv run --with pillow …`) and LXGW WenKai Medium in `~/Library/Fonts`.

## Each release

### 1. Bump the version

`version = "X.Y.Z"` in the root `Cargo.toml`; both crates inherit it. Refresh the lock and commit it with the rest:

```
cargo check -p onyx-app
```

### 2. Write the changelog section

`## vX.Y.Z` at the top of `CHANGELOG.md`, for the person installing the app: a numbered list, a topic in bold and one sentence, small fixes under **Fixes**, ten items at most. `scripts/release-notes.sh X.Y.Z` prints what the release page will show.

### 3. Check it before the tag

Commit the bump and the changelog, then:

```
caffeinate -d -t 14400 &             # the display stays on, so the screen stays unlocked
scripts/release-check.sh X.Y.Z
```

The notary credential is out of reach while the screen is locked (When something fails). An agent starts `caffeinate` as a background command of its own. Do not tag until the check says "ready to tag".

### 4. Push, wait for the tests, then tag

```
git push origin HEAD:main            # starts rust.yml on every runner
gh run list --workflow rust --limit 1
gh run watch <run-id> --exit-status
git tag -a vX.Y.Z -m "Onyx vX.Y.Z"
git push origin refs/tags/vX.Y.Z
```

The tag starts `release.yml`, about twenty minutes, ending in a draft. Watch it with `gh run list --workflow release --limit 3` and `gh run watch`.

### 5. Build, sign and notarize the Mac images here

From the tagged commit, while CI runs:

```
cargo build --release --locked -p onyx-app
cargo build --release --locked -p onyx-app --target x86_64-apple-darwin
scripts/release-mac.sh --no-build --notarize
scripts/release-mac.sh --no-build --notarize --target x86_64-apple-darwin
```

Each run ends with `built dist/Onyx-mac-<arch>.dmg (X.Y.Z, <arch>, signed, notarized)`. Check each run's exit code and last line, not output piped through `tail`.

### 6. Install and run it on Windows and Linux

Skipped while only the Mac is published. Once they are, in VMs, from the draft itself:

```
gh release download vX.Y.Z --dir dist/try --pattern 'Onyx-windows-*-setup.exe' --pattern 'Onyx-linux-arm64.*'
```

- **Windows**: mark each copied installer as downloaded (``Set-Content <file> -Stream Zone.Identifier -Value "[ZoneTransfer]`r`nZoneId=3"``), run it, and see what SmartScreen shows, then the splash and the window. Both architectures (ARM Windows runs x64 emulated).
- **Linux**: `sudo apt install ./Onyx-linux-arm64.deb`, open it from the app grid; make the AppImage executable and run it. A terminal beside each shows no error.
- In each: the window opens, the sidebar slides, the settings open, and the window closed and opened twice comes back where it was. Start an app the person's way (Explorer, the app grid), not from the VM tool's command runner, whose job ends the app with the command.

A package that does not install or open is a reason to stop and redo from step 3.

### 7. Put them on the release and publish it

```
scripts/release-publish.sh X.Y.Z
```

It refuses until the release workflow for the tag has succeeded and both images are notarized, stapled and newer than the tag's commit; then uploads them over CI's, downloads them back and compares checksums, checks them as Gatekeeper will, and publishes the draft as the latest release. The release is not done until it prints "published with notarized Mac images".

### 8. Install the release build here

```
hdiutil attach -nobrowse dist/Onyx-mac-arm64.dmg
ditto "/Volumes/Onyx vX.Y.Z/Onyx.app" /Applications/Onyx.app.new
mv /Applications/Onyx.app "$TMPDIR/Onyx.app.old" && mv /Applications/Onyx.app.new /Applications/Onyx.app
rm -rf "$TMPDIR/Onyx.app.old"
hdiutil detach "/Volumes/Onyx vX.Y.Z"
```

Beside the old bundle and renamed into place, never copied over it: the installed app may be running, and a copy over a running app's executable can end it.

### 9. Delete the compiled files

```
scripts/release-clean.sh
```

Every time, without being asked, once step 7 printed its line and the installed copy opens. It deletes `target/` and `dist/`, fails if Spotlight finds a second `Onyx.app`, and, when the agent is the dev build's child, hands over to the installed copy once the turn is over: say so in the reply.

## Redoing a release under the same tag

Delete the release and the tag on both sides, cancel a run still going, then repeat from step 3 with the new commit; the images are rebuilt, tried and published again:

```
gh release delete vX.Y.Z --yes
git tag -d vX.Y.Z
git push origin :refs/tags/vX.Y.Z
```

An agent's auto mode refuses these as destructive unless the person allowed them; otherwise the person runs them.

## Signing in CI

With five repository secrets, `release.yml` signs and notarizes the Mac images itself; step 5 goes, and step 7 is `gh release edit vX.Y.Z --draft=false --latest`:

```
security export -k login.keychain-db -t identities -f pkcs12 -P "<passphrase>" -o /tmp/cert.p12
gh secret set MACOS_CERTIFICATE_P12 < <(base64 -i /tmp/cert.p12)
gh secret set MACOS_CERTIFICATE_PASSWORD --body "<passphrase>"
gh secret set APPLE_ID --body "<Apple ID email>"
gh secret set APPLE_APP_PASSWORD --body "<app-specific password>"
gh secret set APPLE_TEAM_ID --body "<team id>"
rm /tmp/cert.p12
```

## Signing on Windows

Azure Artifact Signing: an account (Basic), identity validation with a government ID, a Public Trust certificate profile, and an app registration given the Certificate Profile Signer role. Then:

```
gh secret set AZURE_TENANT_ID --body "<tenant id>"
gh secret set AZURE_CLIENT_ID --body "<client id>"
gh secret set AZURE_CLIENT_SECRET --body "<client secret>"
gh variable set ARTIFACT_SIGNING_ENDPOINT --body "https://<region>.codesigning.azure.net/"
gh variable set ARTIFACT_SIGNING_ACCOUNT --body "<account name>"
gh variable set ARTIFACT_SIGNING_PROFILE --body "<certificate profile name>"
```

The client secret expires within two years. `Get-AuthenticodeSignature .\Onyx-windows-x64-setup.exe` says whether an installer is signed.

## The stores

- **Microsoft Store**: reserve the name in Partner Center, then `gh variable set STORE_IDENTITY_NAME`, `STORE_PUBLISHER` and `STORE_PUBLISHER_NAME` from Product identity. After step 7, `gh run download <run-id> --pattern 'store-*' --dir dist/store` and submit both packages; give the reason for `unvirtualizedResources`.
- **Snap Store**: register the name at snapcraft.io and ask for classic confinement on the forum. Then `snapcraft export-login --snaps=onyx --acls package_access,package_push,package_update,package_release - | gh secret set SNAPCRAFT_STORE_CREDENTIALS` and `gh variable set SNAP_CHANNEL --body stable`; every release run uploads both snaps from then on.

## When something fails

Every failure met while cutting a release gets a line here once it is solved, with what would have caught it.

- **"No Keychain password item found for profile"** while the profile is in place: the screen is locked, and the profile's keychain opens only while it is unlocked. Keep the display on (step 3) and notarize while the person is at the Mac.
- **`generate_icon_enum: failed to read` a path that is gone**: the repository's folder was moved or renamed with `target/` in it, and build scripts' saved output still names the old path, which `release-check.sh` finds. `cargo clean`, and let a release build start clean rather than from objects that hold the old path.
- **A test passes here and fails on Windows or Linux**: wait for `rust.yml` before tagging (step 4).
- **`git push origin HEAD:main` is refused as non-fast-forward**: `main` has a commit the release branch does not, such as a fix made on GitHub. Merge `origin/main` into the branch, keeping the branch's side where both changed the same line, and run step 3 again on the merge. `git merge-base --is-ancestor origin/main HEAD` after a `git fetch` finds it before the check.
- **The Windows job cannot find a type**: a line used on every platform leans on something gated for Unix; `release-check.sh` runs the type check that finds it.
- **`hdiutil create` fails with "Resource busy"** on a runner: `release-mac.sh` tries five times; re-run only that job with `gh run rerun <run-id> --failed`.
- **Finder "AppleEvent timed out (-1712)"**: allow the terminal to control Finder (Privacy & Security, Automation) and run again.
- **Notarization rejected**: `xcrun notarytool log <submission-id> --keychain-profile notarytool-profile`; look for a binary without the hardened runtime.
- **The Mac images on the release are ad-hoc signed**: step 7 was skipped. `spctl -a -vv /Applications/Onyx.app` must say "Notarized Developer ID".
- **Two Onyxs in Launchpad or Spotlight**: `mdfind "kMDItemFSName == 'Onyx.app'"`; keep only `/Applications/Onyx.app`.
