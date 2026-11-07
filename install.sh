#!/usr/bin/env bash
# Build omafile from source and wire it into Omarchy.
#
#   ./install.sh            build + install binary and desktop entry (shows up in Walker)
#   ./install.sh --default  also make omafile the default app for folders (xdg-mime)
#   ./install.sh --bind     also replace Nautilus on SUPER+F in ~/.config/hypr/bindings.conf
#   ./install.sh --all      everything above
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin_dir="$HOME/.local/bin"
apps_dir="$HOME/.local/share/applications"
share_dir="$HOME/.local/share/omafile"
bindings="$HOME/.config/hypr/bindings.conf"
desktop="io.github.whogotpwned.Omafile.desktop"

want_default=false
want_bind=false
for arg in "$@"; do
    case "$arg" in
        --default) want_default=true ;;
        --bind) want_bind=true ;;
        --all) want_default=true; want_bind=true ;;
        -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg" >&2; exit 2 ;;
    esac
done

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found. On Omarchy: sudo pacman -S --needed rust" >&2
    exit 1
fi
if ! pkg-config --exists gtk4 libadwaita-1 2>/dev/null; then
    echo "GTK 4 / libadwaita headers missing. On Omarchy: sudo pacman -S --needed gtk4 libadwaita" >&2
    exit 1
fi

echo ":: building omafile (release)"
cargo build --release --manifest-path "$here/Cargo.toml"

mkdir -p "$bin_dir" "$apps_dir" "$share_dir"
install -m755 "$here/target/release/omafile" "$bin_dir/omafile"
install -m644 "$here/data/$desktop" "$apps_dir/$desktop"
install -m644 "$here/data/hyprland-bindings.conf" "$share_dir/hyprland-bindings.conf"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps_dir" || true
echo ":: installed $bin_dir/omafile and $apps_dir/$desktop"
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo "   note: $bin_dir is not on your PATH" ;;
esac

if $want_default; then
    xdg-mime default "$desktop" inode/directory
    echo ":: omafile is now the default application for folders"
fi

if $want_bind; then
    mkdir -p "$(dirname "$bindings")"
    touch "$bindings"
    if grep -q "omafile" "$bindings"; then
        echo ":: $bindings already mentions omafile, leaving it alone"
    else
        {
            echo ""
            echo "# omafile replaces Nautilus as the file manager"
            echo "unbind = SUPER, F"
            echo "bindd = SUPER, F, File manager, exec, uwsm app -- omafile"
        } >> "$bindings"
        echo ":: SUPER+F now opens omafile ($bindings)"
        command -v hyprctl >/dev/null 2>&1 && hyprctl reload >/dev/null 2>&1 || true
    fi
fi

$want_default || echo ":: run with --default to open folders with omafile by default"
$want_bind || echo ":: run with --bind to put omafile on SUPER+F"
