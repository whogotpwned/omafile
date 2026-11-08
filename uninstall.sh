#!/usr/bin/env bash
# Remove everything install.sh put in place. Mirrors install.sh step by step.
#
#   ./uninstall.sh            remove binary, desktop entry, shortcuts, default-app setting
#   ./uninstall.sh --purge    also delete ~/.config/omafile (preferences, tags)
set -euo pipefail

bin_dir="$HOME/.local/bin"
apps_dir="$HOME/.local/share/applications"
share_dir="$HOME/.local/share/omafile"
bindings_lua="$HOME/.config/hypr/bindings.lua"
bindings_conf="$HOME/.config/hypr/bindings.conf"
desktop="io.github.whogotpwned.Omafile.desktop"
purge=false
[ "${1:-}" = "--purge" ] && purge=true

rm -f "$bin_dir/omafile" "$apps_dir/$desktop"
rm -f "$HOME/.local/share/icons/hicolor/scalable/apps/io.github.whogotpwned.Omafile.svg" \
      "$HOME/.local/share/icons/hicolor/symbolic/apps/io.github.whogotpwned.Omafile-symbolic.svg"
rm -rf "$share_dir"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$apps_dir" || true
echo ":: removed binary and desktop entry"

# Hand folders back to Nautilus if omafile was the default.
if command -v xdg-mime >/dev/null 2>&1 && [ "$(xdg-mime query default inode/directory 2>/dev/null)" = "$desktop" ]; then
    xdg-mime default org.gnome.Nautilus.desktop inode/directory 2>/dev/null || true
    echo ":: folders open with Nautilus again"
fi

# Drop the binding lines install.sh appended (only those).
strip_block() {
    local file="$1" marker="$2"
    [ -f "$file" ] || return 0
    grep -q "$marker" "$file" || return 0
    local tmp
    tmp="$(mktemp)"
    awk -v m="$marker" '
        $0 == m { skip = 1; next }
        skip && /omafile/ { next }
        { skip = 0; print }
    ' "$file" > "$tmp"
    mv "$tmp" "$file"
    echo ":: removed omafile shortcuts from $file"
}
strip_block "$bindings_lua" "-- omafile replaces Nautilus as the file manager"
strip_block "$bindings_conf" "# omafile replaces Nautilus as the file manager"
command -v hyprctl >/dev/null 2>&1 && hyprctl reload >/dev/null 2>&1 || true

if $purge; then
    rm -rf "$HOME/.config/omafile"
    echo ":: removed ~/.config/omafile"
else
    echo ":: kept ~/.config/omafile (run with --purge to delete preferences and tags)"
fi

if command -v omarchy >/dev/null 2>&1 && [ -d "$HOME/.config/omarchy/plugins/whogotpwned.omafile" ]; then
    echo ":: the bar button is still installed; remove it with:"
    echo "   omarchy plugin remove whogotpwned.omafile"
fi
