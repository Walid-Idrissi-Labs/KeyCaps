#!/bin/sh
# Installs Keycaps for this user, without root: the program in ~/.local/bin, its sounds in
# ~/.local/share/keycaps, an entry in the applications menu, and one that starts it with
# the desktop.   ./install.sh          install, or update, and start
#                ./install.sh remove   stop and remove (settings stay in ~/.config/keycaps)
set -e
cd "$(dirname "$0")"
bin="$HOME/.local/bin"
share="$HOME/.local/share"
entries="$share/applications/keycaps.desktop ${XDG_CONFIG_HOME:-$HOME/.config}/autostart/keycaps.desktop"
icon="$share/icons/hicolor/256x256/apps/keycaps.png"

[ -x "$bin/keycaps" ] && "$bin/keycaps" quit >/dev/null 2>&1 || true
if [ "$1" = remove ]; then
    rm -rf "$bin/keycaps" "$share/keycaps" "$icon" $entries
    echo "Keycaps is removed."
    exit
fi

mkdir -p "$bin" "$share/keycaps" "$(dirname "$icon")"
cp keycaps "$bin/keycaps"
chmod 755 "$bin/keycaps"
rm -rf "$share/keycaps/sounds"
cp -R sounds LICENSE "$share/keycaps/"
cp keycaps.png "$icon"
for entry in $entries; do
    mkdir -p "$(dirname "$entry")"
    cat > "$entry" <<END
[Desktop Entry]
Type=Application
Name=Keycaps
Comment=The sound of a mechanical keyboard as you type
Exec=$bin/keycaps
Icon=keycaps
Terminal=false
Categories=Utility;
END
done

# Keys are read from /dev/input, which is open to the group "input" only.
if id -nG | tr ' ' '\n' | grep -qx input; then
    setsid "$bin/keycaps" >/dev/null 2>&1 &
    echo "Keycaps is installed and running. Look for the keyboard in the tray, or run: keycaps help"
else
    echo "Keycaps is installed. One step is left, because it listens to the keyboard:"
    echo "    sudo usermod -aG input $(id -un)"
    echo "then log out and back in. Keycaps starts with the desktop from then on."
fi
case ":$PATH:" in *":$bin:"*) ;; *) echo "($bin is not on your PATH: call it as $bin/keycaps)" ;; esac
