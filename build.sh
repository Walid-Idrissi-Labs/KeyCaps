#!/bin/sh
# ./build.sh           Keycaps.app for this Mac, next to this script
# ./build.sh release   dist/Keycaps.dmg for Apple silicon and Intel, as published
set -e
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET=11.0 # LSMinimumSystemVersion in Info.plist
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
if [ "$1" = release ]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo build --release --target aarch64-apple-darwin --target x86_64-apple-darwin
    binary=target/keycaps-universal
    lipo -create -output $binary target/aarch64-apple-darwin/release/keycaps target/x86_64-apple-darwin/release/keycaps
    rm -rf dist
    bundle=dist/Keycaps/Keycaps.app # dist/Keycaps becomes the disk image
    id=- # ad hoc: the same signature whoever builds it
else
    cargo build --release
    binary=target/release/keycaps
    bundle=Keycaps.app
    # Signed with the local certificate if there is one, the app keeps its identity across
    # builds, and with it the Input Monitoring grant. Ad hoc, every build has to ask again.
    id="Keycaps Local Signing"
    security find-identity -v -p codesigning | grep -q "\"$id\"" || id=-
fi
app=$bundle/Contents
mkdir -p $app/MacOS $app/Resources
rm -f $app/MacOS/keycaps # a running copy keeps the file it was started from
cp $binary $app/MacOS/keycaps
cp Info.plist $app/
plutil -replace CFBundleShortVersionString -string "$version" $app/Info.plist
plutil -replace CFBundleVersion -string "$version" $app/Info.plist
cp LICENSE $app/Resources/
cp assets/icons/industrial/Keycaps-Titanium-K.icns $app/Resources/Keycaps.icns
cp assets/icons/industrial/menubar-titanium-k/KeycapsMenuBar.pdf $app/Resources/
rsync -a --delete sounds/ $app/Resources/sounds/
codesign --force --sign "$id" $bundle
if [ "$1" = release ]; then
    ln -s /Applications dist/Keycaps/Applications
    hdiutil create -quiet -volname Keycaps -srcfolder dist/Keycaps -format UDZO dist/Keycaps.dmg
fi
