#!/usr/bin/env bash
# Build omafile from source and wire it into Omarchy.
#
#   ./install.sh            build + install binary and desktop entry (shows up in the launcher)
#   ./install.sh --default  also make omafile the default app for folders (xdg-mime)
#   ./install.sh --bind     also replace Nautilus on SUPER+SHIFT+F (Lua config) / SUPER+F (old config)
#   ./install.sh --all      everything above
#
# The Omarchy shell plugin (bar button) is added separately:
#   omarchy plugin add https://github.com/whogotpwned/omafile.git --enable
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin_dir="$HOME/.local/bin"
apps_dir="$HOME/.local/share/applications"
share_dir="$HOME/.local/share/omafile"
bindings_lua="$HOME/.config/hypr/bindings.lua"
bindings_conf="$HOME/.config/hypr/bindings.conf"
desktop="io.github.whogotpwned.Omafile.desktop"

want_default=false
want_bind=false
for arg in "$@"; do
    case "$arg" in
        --default) want_default=true ;;
        --bind) want_bind=true ;;
        --all) want_default=true; want_bind=true ;;
        -h|--help) sed -n '2,11p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found. On Omarchy: sudo pacman -S --needed rust" >&2
    exit 1
fi
if ! pkg-config --exists gtk4 libadwaita-1 vte-2.91-gtk4 2>/dev/null; then
    echo "GTK 4 / libadwaita / VTE headers missing. On Omarchy: sudo pacman -S --needed gtk4 libadwaita vte4" >&2
    exit 1
fi

echo ":: building omafile (release)"
cargo build --release --manifest-path "$here/Cargo.toml"

icons_dir="$HOME/.local/share/icons/hicolor"
mkdir -p "$bin_dir" "$apps_dir" "$share_dir" "$icons_dir/scalable/apps" "$icons_dir/symbolic/apps"
install -m755 "$here/target/release/omafile" "$bin_dir/omafile"
install -m644 "$here/data/$desktop" "$apps_dir/$desktop"
install -m644 "$here/data/icons/io.github.whogotpwned.Omafile.svg" "$icons_dir/scalable/apps/"
install -m644 "$here/data/icons/io.github.whogotpwned.Omafile-symbolic.svg" "$icons_dir/symbolic/apps/"
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q -t "$icons_dir" 2>/dev/null || true
install -m644 "$here/data/hyprland-bindings.lua" "$share_dir/hyprland-bindings.lua"
install -m644 "$here/data/hyprland-bindings.conf" "$share_dir/hyprland-bindings.conf"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps_dir" || true
echo ":: installed $bin_dir/omafile and $apps_dir/$desktop"
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo "   note: $bin_dir is not on your PATH" ;;
esac

echo ":: optional tools"
for pair in "pdftoppm:PDF previews (poppler)" "fd:fast recursive search" "bsdtar:archives (libarchive)" "git:git status badges" "gocryptfs:encrypted folders (sudo pacman -S gocryptfs)" "script:master key display for new vaults (util-linux)" "magick:image tools: rotate, convert, resize (imagemagick)" "qrencode:QR code for local network sharing" "python3:local network sharing (http.server)"; do
    tool="${pair%%:*}"; what="${pair#*:}"
    if command -v "$tool" >/dev/null 2>&1; then echo "   ok       $tool  ($what)"; else echo "   missing  $tool  ($what)"; fi
done

if $want_default; then
    xdg-mime default "$desktop" inode/directory
    echo ":: omafile is now the default application for folders"
fi

if $want_bind; then
    if [ -f "$bindings_lua" ] || [ -f "$HOME/.config/hypr/hyprland.lua" ]; then
        touch "$bindings_lua"
        if grep -q "omafile" "$bindings_lua"; then
            echo ":: $bindings_lua already mentions omafile, leaving it alone"
        else
            {
                echo ""
                echo "-- omafile replaces Nautilus as the file manager"
                echo 'o.rebind("SUPER + SHIFT + F", "File manager", { launch = "omafile" })'
                echo 'o.rebind("SUPER + ALT + SHIFT + F", "File manager (cwd)", "setsid uwsm-app -- omafile \"$(omarchy-cmd-terminal-cwd)\"")'
            } >> "$bindings_lua"
            echo ":: SUPER+SHIFT+F now opens omafile ($bindings_lua)"
        fi
    else
        mkdir -p "$(dirname "$bindings_conf")"
        touch "$bindings_conf"
        if grep -q "omafile" "$bindings_conf"; then
            echo ":: $bindings_conf already mentions omafile, leaving it alone"
        else
            {
                echo ""
                echo "# omafile replaces Nautilus as the file manager"
                echo "unbind = SUPER, F"
                echo "bindd = SUPER, F, File manager, exec, uwsm app -- omafile"
            } >> "$bindings_conf"
            echo ":: SUPER+F now opens omafile ($bindings_conf)"
        fi
    fi
    command -v hyprctl >/dev/null 2>&1 && hyprctl reload >/dev/null 2>&1 || true
fi

$want_default || echo ":: run with --default to open folders with omafile by default"
$want_bind || echo ":: run with --bind to put omafile on the file manager shortcut"
cat <<MSG
:: bar button (Omarchy shell plugin):
   omarchy plugin add https://github.com/whogotpwned/omafile.git --enable
   omarchy bar move whogotpwned.omafile --section right   # or left / centerBegin / centerEnd / tray
MSG
