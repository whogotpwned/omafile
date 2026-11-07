#!/usr/bin/env bash
# Build omafile from source and wire it into Omarchy.
#
#   ./install.sh          build + install binary, desktop entry, shell helper
#   ./install.sh --bind   additionally add SUPER+SHIFT+F to ~/.config/hypr/bindings.conf
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
bin_dir="$HOME/.local/bin"
share_dir="$HOME/.local/share/omafile"
apps_dir="$HOME/.local/share/applications"
bindings="$HOME/.config/hypr/bindings.conf"

if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo not found. On Omarchy/Arch: sudo pacman -S --needed rust" >&2
    exit 1
fi

echo ":: building omafile (release)"
cargo build --release --manifest-path "$here/Cargo.toml"

mkdir -p "$bin_dir" "$share_dir" "$apps_dir"
install -m755 "$here/target/release/omafile" "$bin_dir/omafile"
install -m644 "$here/contrib/omafile.desktop" "$apps_dir/omafile.desktop"
install -m644 "$here/contrib/shell-integration.sh" "$share_dir/shell-integration.sh"
install -m644 "$here/contrib/hyprland-bindings.conf" "$share_dir/hyprland-bindings.conf"
if [ -d "$HOME/.config/fish" ]; then
    mkdir -p "$HOME/.config/fish/functions"
    install -m644 "$here/contrib/shell-integration.fish" "$HOME/.config/fish/functions/of.fish"
fi
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps_dir" || true

echo ":: installed $bin_dir/omafile"
case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo "   note: $bin_dir is not on your PATH" ;;
esac

if [ "${1:-}" = "--bind" ]; then
    mkdir -p "$(dirname "$bindings")"
    touch "$bindings"
    if grep -q "omafile" "$bindings"; then
        echo ":: $bindings already mentions omafile, skipping"
    else
        {
            echo ""
            echo "# omafile"
            echo "bindd = SUPER SHIFT, F, File manager (omafile), exec, uwsm app -- alacritty --class Omafile -e omafile"
        } >> "$bindings"
        echo ":: added SUPER+SHIFT+F to $bindings"
        command -v hyprctl >/dev/null 2>&1 && hyprctl reload >/dev/null 2>&1 || true
    fi
else
    echo ":: run with --bind to add SUPER+SHIFT+F to $bindings"
fi

cat <<MSG

Shell integration (cd to the directory you quit in), add to ~/.bashrc or ~/.zshrc:
    source $share_dir/shell-integration.sh
then use:  of [path]
MSG
