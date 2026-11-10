#!/usr/bin/env bash
# Push both packages to the AUR. Needs an AUR account whose SSH key is
# registered (https://aur.archlinux.org/account), run once per release after
# packaging/aur/update.sh.
#   packaging/aur/publish.sh            both packages
#   packaging/aur/publish.sh omafile    just one
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
packages=("${@:-omafile omafile-bin}")
[ $# -gt 0 ] || packages=(omafile omafile-bin)
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
for pkg in "${packages[@]}"; do
    src="$here/$pkg"
    [ -f "$src/PKGBUILD" ] && [ -f "$src/.SRCINFO" ] || { echo "$pkg: run packaging/aur/update.sh first" >&2; exit 1; }
    if grep -q "SKIP\|REPLACE_ME" "$src/PKGBUILD"; then
        echo "$pkg: checksums are not filled in yet (packaging/aur/update.sh)" >&2
        exit 1
    fi
    ver="$(sed -n 's/^pkgver=//p' "$src/PKGBUILD")-$(sed -n 's/^pkgrel=//p' "$src/PKGBUILD")"
    echo ":: $pkg $ver"
    git clone -q "ssh://aur@aur.archlinux.org/$pkg.git" "$work/$pkg"
    cp "$src/PKGBUILD" "$src/.SRCINFO" "$work/$pkg/"
    git -C "$work/$pkg" add PKGBUILD .SRCINFO
    if git -C "$work/$pkg" diff --cached --quiet; then
        echo "   nothing to publish"
        continue
    fi
    git -C "$work/$pkg" commit -q -m "Update to $ver"
    git -C "$work/$pkg" push -q origin HEAD:master
    echo "   pushed: https://aur.archlinux.org/packages/$pkg"
done
