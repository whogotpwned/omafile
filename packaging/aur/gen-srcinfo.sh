#!/usr/bin/env bash
# Write .SRCINFO next to a PKGBUILD. Uses makepkg when it is there (the
# authoritative way); otherwise a small generator that understands the plain
# PKGBUILDs in this repository, so the file can be refreshed on any machine.
#   packaging/aur/gen-srcinfo.sh path/to/PKGBUILD
set -euo pipefail
pkgbuild="$(realpath "${1:?usage: gen-srcinfo.sh path/to/PKGBUILD}")"
dir="$(dirname "$pkgbuild")"
if command -v makepkg >/dev/null 2>&1 && [ "$(id -u)" != 0 ]; then
    (cd "$dir" && makepkg --printsrcinfo -p "$(basename "$pkgbuild")" > .SRCINFO)
    echo "$dir/.SRCINFO (makepkg)"
    exit 0
fi
(
    set +u
    CARCH="${CARCH:-x86_64}"
    # shellcheck disable=SC1090
    source "$pkgbuild"
    echo "pkgbase = $pkgname"
    echo "	pkgdesc = $pkgdesc"
    echo "	pkgver = $pkgver"
    echo "	pkgrel = $pkgrel"
    echo "	url = $url"
    for a in "${arch[@]}"; do echo "	arch = $a"; done
    for l in "${license[@]}"; do echo "	license = $l"; done
    for d in "${checkdepends[@]}"; do echo "	checkdepends = $d"; done
    for d in "${makedepends[@]}"; do echo "	makedepends = $d"; done
    for d in "${depends[@]}"; do echo "	depends = $d"; done
    for d in "${optdepends[@]}"; do echo "	optdepends = $d"; done
    for d in "${provides[@]}"; do echo "	provides = $d"; done
    for d in "${conflicts[@]}"; do echo "	conflicts = $d"; done
    for s in "${source[@]}"; do echo "	source = $s"; done
    for s in "${sha256sums[@]}"; do echo "	sha256sums = $s"; done
    for s in "${source_x86_64[@]}"; do echo "	source_x86_64 = $s"; done
    for s in "${sha256sums_x86_64[@]}"; do echo "	sha256sums_x86_64 = $s"; done
    echo
    echo "pkgname = $pkgname"
) > "$dir/.SRCINFO"
echo "$dir/.SRCINFO (built-in generator)"
