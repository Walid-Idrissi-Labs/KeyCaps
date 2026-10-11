# Releasing Keycaps

## Making a release

1. Set the new version in `Cargo.toml`. The builds stamp it into the app.
2. Commit, tag the commit `vX.Y.Z` and push the tag.
3. CI runs the tests on macOS and on Linux, builds the release files with `./build.sh release` and drafts a GitHub release that carries them. It fails if the tag and the version in `Cargo.toml` disagree.
4. Download the files from the draft, try them, write the notes and publish.

The README links to `releases/latest/download/Keycaps.dmg` and `keycaps-linux-<cpu>.tar.gz`, so its buttons follow the newest published release by themselves. That is why the file names carry no version.

## What a release is

### macOS: `Keycaps.dmg`

One app for Apple silicon and Intel (macOS 11 or later), in a disk image with a shortcut to Applications. The app is signed ad hoc and is not notarized, which has two consequences for the people who download it:

- macOS blocks the first launch until they allow it under System Settings → Privacy & Security. The README explains how.
- macOS treats every version as a new app, so Input Monitoring has to be allowed again after each update. The entry of the previous version would otherwise stay switched on while applying to nothing, so the first launch of a build that finds itself without access clears it (`forget_stale_permission` in `src/mac.rs`), and macOS asks again.

Both go away with an Apple Developer ID: sign with it in `build.sh` using the hardened runtime, submit the image with `xcrun notarytool`, and staple the ticket.

### Linux: `keycaps-linux-x86_64.tar.gz` and `keycaps-linux-aarch64.tar.gz`

A folder with the program, the recordings and `install.sh`, which installs for one user without root. The program is linked statically, with the linker that ships with Rust (`.cargo/config.toml`), so one file runs on every distribution and `./build.sh linux` can make both archives on a Mac as well.

## Local builds

`./build.sh` builds for the machine it runs on: `Keycaps.app` on a Mac, `dist/keycaps` on Linux.

On a Mac, if the keychain holds a code-signing certificate named "Keycaps Local Signing", the app is signed with it and keeps its Input Monitoring permission from one build to the next. Without it, the build is signed ad hoc and macOS asks after every build. A release build and a local build share the bundle identifier `local.keycaps`, and macOS keeps one Input Monitoring entry for it, so opening one after the other on the same Mac makes macOS ask again.

## Tests

- `cargo test` runs the unit tests of the system it runs on.
- `tests/linux/run.sh <folder>` runs a Linux build against a real sound server, a virtual keyboard and mouse, and a stand-in tray. CI does this on every push. On a Mac it runs in a container: Docker's Linux kernel can make the virtual devices if the container is privileged and sees `/dev/input`.

## Artwork

- The app icon and the menu bar symbol are in `assets/icons/industrial`. `build.sh` names the files it copies.
- `python3 scripts/readme-art.py` redraws the pictures in the README.
