#!/bin/sh
# End-to-end test of a Linux build, on a real kernel and a real sound server:
#
#     tests/linux/run.sh dist/linux-x86_64/keycaps
#
# It starts a private PulseAudio whose silent output is recorded, plays the part of a tray,
# and with sudo also makes a virtual keyboard and mouse to type on. Needs pulseaudio,
# pulseaudio-utils, dbus, and python3 with python3-dbus, python3-gi and
# gir1.2-dbusmenu-glib-0.4. Nothing of the user's own session or settings is touched.
set -e
app=$(cd "$1" && pwd)/keycaps
here=$(cd "$(dirname "$0")" && pwd)
if [ -z "$KEYCAPS_TEST" ]; then
    KEYCAPS_TEST=$(mktemp -d)
    export KEYCAPS_TEST HOME=$KEYCAPS_TEST/home XDG_RUNTIME_DIR=$KEYCAPS_TEST/run
    unset XDG_CONFIG_HOME XDG_STATE_HOME PULSE_SERVER PULSE_COOKIE
    mkdir -p "$HOME" "$XDG_RUNTIME_DIR"
    chmod 700 "$XDG_RUNTIME_DIR"
    exec dbus-run-session -- "$0" "$1"
fi
tmp=$KEYCAPS_TEST
cleanup() {
    "$app" quit > /dev/null 2>&1 || true
    [ -z "$recorder" ] || kill $recorder 2> /dev/null || true
    [ -z "$typist" ] || sudo kill $typist 2> /dev/null || true
    pulseaudio --kill 2> /dev/null || true
    rm -rf "$tmp"
}
trap cleanup EXIT

fail() { echo "FAILED: $1"; cat "$tmp/keycaps.log"; exit 1; }
expect() { echo "$2" | grep -q -- "$3" || fail "$1: expected \"$3\" in: $2"; echo "  ok    $1"; }
record() { parecord --device=tap.monitor --rate=48000 --channels=2 --format=s16le --raw --latency-msec=20 "$tmp/sound.raw" & recorder=$!; sleep 0.8; }
heard() { sleep 0.6; kill -INT $recorder; wait $recorder 2>/dev/null || true; python3 "$here/sounds.py" "$tmp/sound.raw" "$2" || fail "$1"; echo "  ok    $1"; }

# A sound server of its own, with an output that never rewrites what it has played, so
# that the recording of it is faithful.
pulseaudio -n --daemonize=yes --exit-idle-time=-1 --load=module-native-protocol-unix \
    --load="module-null-sink sink_name=tap norewinds=1 rate=48000 channels=2 format=s16le"
pactl set-default-sink tap

"$app" > "$tmp/keycaps.log" 2>&1 &
sleep 0.5
expect "it starts with the first-run settings" "$("$app" status)" "switch cream"

echo "sound"
record; "$app" volume 60; heard "a volume command plays a key in the middle" "C"
record; "$app" spatial wide; heard "a spatial command plays one at the left edge" "L"
record; "$app" mouse apple-magic-mouse; heard "choosing a mouse plays its click" "C"
"$app" spatial natural; "$app" mouse off; sleep 0.3

if sudo -n true 2>/dev/null && [ -c /dev/uinput ]; then
    echo "keyboard and mouse"
    mkfifo "$tmp/keys"
    sudo python3 "$here/typist.py" < "$tmp/keys" > "$tmp/typist.log" & typist=$!
    exec 3> "$tmp/keys"
    # Sends commands to the typist and waits until each has been answered.
    type() { want=$(($(wc -l < "$tmp/typist.log") + $#)); printf '%s\n' "$@" >&3; while [ "$(wc -l < "$tmp/typist.log")" -lt $want ]; do sleep 0.05; done; }
    until grep -q ready "$tmp/typist.log"; do sleep 0.1; done
    expect "devices it may not read are reported" "$("$app" status)" "Cannot hear the keyboard"
    type open
    expect "and found as soon as they can be read" "$("$app" status)" "Listening to 1 input device"
    record; type "key 30" "sleep 400" "key 38" "sleep 400" "key 57" "sleep 400" "hold 31 300" "sleep 300"
    heard "A is left, L right, space in the middle, and a held S sounds once and once more on release" "L R C S L"
    expect "four keys were counted, the repeats not" "$("$app" status)" " 4 keys today"
    record; type "click 0x110" "move 5 5" "wheel 1" "sleep 300"; heard "the mouse is silent while it is off" ""
    "$app" mouse apple-magic-mouse; "$app" scroll on; sleep 0.5
    expect "the mouse is listened to once it is on" "$("$app" status)" "Listening to 2 input devices"
    record; type "click 0x110" "sleep 400" "click 0x111" "sleep 400" "wheel 1" "sleep 400" "move 9 9" "sleep 200"
    heard "left click, right click and a wheel tick sound, movement does not" "C C C"
    "$app" off
    record; type "key 30" "sleep 200" "click 0x110" "sleep 200"; heard "nothing sounds while Keycaps is off" ""
    "$app" on; type unplug
    expect "unplugged devices are let go" "$("$app" status)" "Listening to 0 input devices"
    type plug open
    record; type "key 38" "sleep 300"; heard "and heard again when plugged back in" "R"
    "$app" mouse off; "$app" scroll off
else
    echo "keyboard and mouse: SKIPPED (needs sudo and /dev/uinput)"
fi

echo "commands"
"$app" switch holypanda; "$app" keyup off
expect "settings are kept in a file" "$(cat "$HOME/.config/keycaps/settings")" "switch holypanda"
expect "a wrong name is refused" "$("$app" switch nonsense || true)" "there is no"
expect "a wrong volume is refused" "$("$app" volume 300 || true)" "from 0 to 100"
"$app" quit; sleep 0.3
"$app" > "$tmp/keycaps.log" 2>&1 &
sleep 0.5
expect "and read back at the next start" "$("$app" status)" "keyup off"
"$app" switch cream

echo "tray"
python3 "$here/tray.py" "$app" || fail "the tray"
echo "all passed"
