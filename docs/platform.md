# Platforms

How Onyx works on macOS, Windows and Linux, each on Intel-class and ARM machines, and what is gated. The studio's constitution has the reasoning; this is what Onyx itself does.

## Rules

- **Everything Unix-only is gated with a stated fallback.** Onyx's own code has no `cfg(unix)` of its own: the vault is `std::fs`, the trash is the `trash` crate (the Finder's, the Recycle Bin, the XDG trash), the watcher the `notify` crate (FSEvents, ReadDirectoryChangesW, inotify). The per-system calls are the template's, in `sys.rs`: open a file or a link, reveal a path (Finder, Explorer, else the folder opened), the window's ground, the languages, a picture of the window for a probe (macOS only; elsewhere it says so), and the traffic lights' fold (`sys::fold_lights`: on a Mac the main window's buttons are moved between gpui's row and a column at the rail's head as the sidebar folds, through AppKit's standard window buttons, and watched so gpui's own placing at a resize or a change of key window is undone at once; nothing in full screen, nor until the buttons have stood in a row once, which is where their pitch is read; elsewhere nothing, and the window's own buttons stay where the system draws them).
- **The system's spelling and printing are the Mac's alone** (`sys::spelling`, `spelling_fixes`, `print_pdf`). English is checked in the process on every system; elsewhere the other languages mark nothing, and a note to print opens in the system's PDF viewer.
- **A note on paper is the same on every system.** Typst is all Rust and the carried faces go with it. Only what a note's own fonts lack differs: `fonts::system_fallbacks` names each system's, and `fontdb` finds them by name, and the machine's fonts named in the settings too (Monaco on a Mac).
- **Every child process starts through `onyx_core::process::command`**, so none opens a console window on Windows. Onyx starts none of its own today; the update does.
- **The window's place is saved from `window_bounds()`**, never `bounds()` (`ui_state.rs`).
- **A key is named as the Mac has it and bound as `secondary-<key>`**; off a Mac the caps say Ctrl and Alt.
- **A note's line ends are plain.** A file with Windows' is read as plain ones and written back plain (`lines::Doc::from_text`).

## macOS

The bundle is cargo-packager's with `macos/Info.plist` merged in: the languages, and Markdown as a document type Onyx edits, so Finder offers Onyx for a `.md` file. The dev build is a bundle of its own in `target/debug/dev.noindex/` (`scripts/dev-app.sh`). The installer is a notarized disk image (`docs/release.md`).

### iCloud Drive

A vault made in iCloud goes in Onyx's own iCloud container when the bundle has one: the folder Finder shows in iCloud Drive as an "App Library" named Onyx with the app's icon on it, as Obsidian's (`iCloud~md~obsidian`) is. The container is asked of the system at launch on a thread of its own (`sys::warm_icloud`, `NSFileManager.URLForUbiquityContainerIdentifier`, which can take a moment the first time and is never asked on the main thread), and its `Documents` folder, the part iCloud Drive shows, is where vaults go (`Shell::cloud_place`). The container exists only for a bundle signed with the iCloud entitlements (`macos/Onyx.entitlements`: the container `iCloud.com.alkalinesoftware.onyx`, CloudDocuments) and a Developer ID provisioning profile that names the container, embedded as `Contents/embedded.provisionprofile`; `Info.plist` names the container under `NSUbiquitousContainers`, public in document scope, so iCloud Drive lists it. `release-mac.sh` does both when `ONYX_PROVISION_PROFILE` is set (`docs/release.md`, One-time setup). A bundle without the profile, the dev build among them, cannot carry the entitlements (the system kills such a bundle at launch), so there the container is `None` and a vault made in iCloud is a plain folder under `~/Library/Mobile Documents/com~apple~CloudDocs/Onyx/`, which the system syncs like any other; a vault made there stays where it is and opens as before once the container exists.

## Windows and Linux

As the template has them: Velopack's installer and the Store's `.msix` on Windows, a `.deb`, an AppImage and a snap on Linux, the window's own buttons where the system draws none (`csd.rs`). Neither has been tried on a machine yet for Onyx, and no release carries them; the release's step 6 tries them in VMs once one does.
