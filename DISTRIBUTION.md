# Releasing Keycaps

## Making a release

1. Set the new version in `Cargo.toml`. The build stamps it into the app.
2. Commit, tag the commit `vX.Y.Z` and push the tag.
3. CI runs the tests, builds `dist/Keycaps.dmg` with `./build.sh release` and drafts a GitHub release that carries it. It fails if the tag and the version in `Cargo.toml` disagree.
4. Download the image from the draft, try it, write the notes and publish.

The download button in the README points at `releases/latest/download/Keycaps.dmg`, so it follows the newest published release by itself.

## What a release is

`./build.sh release` builds one app for Apple silicon and Intel (macOS 11 or later) and packs it into a disk image with a shortcut to Applications. The app is signed ad hoc and is not notarized, which has two consequences for the people who download it:

- macOS blocks the first launch until they allow it under System Settings → Privacy & Security. The README explains how.
- macOS treats every version as a new app, so Input Monitoring has to be allowed again after each update. The entry of the previous version would otherwise stay switched on while applying to nothing, so the first launch of a build that finds itself without access clears it (`forget_stale_permission` in `src/main.rs`), and macOS asks again.

Both go away with an Apple Developer ID: sign with it in `build.sh` using the hardened runtime, submit the image with `xcrun notarytool`, and staple the ticket.

## Local builds

`./build.sh` builds `Keycaps.app` for the Mac it runs on. If the keychain holds a code-signing certificate named "Keycaps Local Signing", the app is signed with it and keeps its Input Monitoring permission from one build to the next. Without it, the build is signed ad hoc and macOS asks after every build.

A release build and a local build share the bundle identifier `local.keycaps`, and macOS keeps one Input Monitoring entry for it. Opening one after the other on the same Mac therefore makes macOS ask again.

## Artwork

- The app icon and the menu bar symbol are in `assets/icons/industrial`. `build.sh` names the two files it copies into the app.
- `python3 scripts/readme-art.py` redraws the pictures in the README.
