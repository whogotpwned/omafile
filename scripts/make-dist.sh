#!/usr/bin/env bash
# Build the binary release tarball that GitHub Releases and the omafile-bin
# AUR package use. Layout inside: omafile-<version>/{bin,share/...} = /usr.
#
#   scripts/make-dist.sh            builds target/release first if needed
#   scripts/make-dist.sh --no-build use the binary that is already there
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"
version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
arch="$(uname -m)"
[ "${1:-}" = "--no-build" ] || cargo build --release --locked
[ -x target/release/omafile ] || { echo "target/release/omafile is missing" >&2; exit 1; }

name="omafile-$version"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
root="$stage/$name"
install -Dm755 target/release/omafile "$root/bin/omafile"
install -Dm644 data/io.github.whogotpwned.Omafile.desktop "$root/share/applications/io.github.whogotpwned.Omafile.desktop"
install -Dm644 data/icons/io.github.whogotpwned.Omafile.svg "$root/share/icons/hicolor/scalable/apps/io.github.whogotpwned.Omafile.svg"
install -Dm644 data/icons/io.github.whogotpwned.Omafile-symbolic.svg "$root/share/icons/hicolor/symbolic/apps/io.github.whogotpwned.Omafile-symbolic.svg"
install -Dm644 data/hyprland-bindings.lua "$root/share/omafile/hyprland-bindings.lua"
install -Dm644 data/hyprland-bindings.conf "$root/share/omafile/hyprland-bindings.conf"
install -Dm644 manifest.json "$root/share/omafile/plugin/manifest.json"
install -Dm644 plugin/BarWidget.qml "$root/share/omafile/plugin/plugin/BarWidget.qml"
for f in plugin/i18n/*.json; do install -Dm644 "$f" "$root/share/omafile/plugin/plugin/i18n/$(basename "$f")"; done
install -Dm644 LICENSE "$root/share/licenses/omafile/LICENSE"
install -Dm644 README.md "$root/share/doc/omafile/README.md"
install -Dm644 CHANGELOG.md "$root/share/doc/omafile/CHANGELOG.md"

mkdir -p dist
out="dist/$name-$arch.tar.gz"
# Reproducible: fixed order, owner and mtime.
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime="@${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct 2>/dev/null || date +%s)}" -C "$stage" -czf "$out" "$name"
(cd dist && sha256sum "$name-$arch.tar.gz" > "$name-$arch.tar.gz.sha256")
echo "$out"
cat "dist/$name-$arch.tar.gz.sha256"
