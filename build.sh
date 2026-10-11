#!/bin/sh
# ./build.sh           for the machine it runs on: Keycaps.app on a Mac, dist/keycaps on Linux
# ./build.sh release   what a release carries: on a Mac dist/Keycaps.dmg (Apple silicon and
#                      Intel), on Linux dist/keycaps-linux-<cpu>.tar.gz (x86_64 and aarch64)
# ./build.sh linux     the Linux release files, from a Mac
set -e
cd "$(dirname "$0")"
export PATH="$HOME/.cargo/bin:$PATH"

if [ "$(uname)" = Linux ] || [ "$1" = linux ]; then
    # A folder that runs as it is, and installs with its install.sh.
    package() {
        rm -rf "$2"
        mkdir -p "$2"
        cp "$1" "$2/keycaps"
        cp LICENSE linux/install.sh "$2/"
        cp assets/icons/industrial/Keycaps-Titanium-K.iconset/icon_256x256.png "$2/keycaps.png"
        cp -R sounds "$2/sounds"
    }
    if [ -z "$1" ]; then
        cargo build --release
        package target/release/keycaps dist/keycaps
        exit
    fi
    # Static, so that one file runs on every distribution.
    for cpu in x86_64 aarch64; do
        rustup target add $cpu-unknown-linux-musl
        cargo build --release --target $cpu-unknown-linux-musl
        package target/$cpu-unknown-linux-musl/release/keycaps dist/linux-$cpu/keycaps
        # The variable and the flag keep a Mac's tar from adding files and attributes of its own.
        COPYFILE_DISABLE=1 tar --no-xattrs -C dist/linux-$cpu -czf dist/keycaps-linux-$cpu.tar.gz keycaps
    done
    exit
fi

export MACOSX_DEPLOYMENT_TARGET=11.0 # LSMinimumSystemVersion in Info.plist
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
if [ "$1" = release ]; then
    rustup target add aarch64-apple-darwin x86_64-apple-darwin
    cargo build --release --target aarch64-apple-darwin --target x86_64-apple-darwin
    binary=target/keycaps-universal
    lipo -create -output $binary target/aarch64-apple-darwin/release/keycaps target/x86_64-apple-darwin/release/keycaps
    rm -rf dist/Keycaps dist/Keycaps.dmg
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
