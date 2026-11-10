#!/usr/bin/env bash
# After `git tag vX.Y.Z && git push --tags` and once the release workflow has
# uploaded its tarball: fill in the checksums of both AUR packages and
# refresh their .SRCINFO.
#   packaging/aur/update.sh            version from Cargo.toml
#   packaging/aur/update.sh 0.18.0
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$here"
version="${1:-$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)}"
repo="https://github.com/whogotpwned/omafile"
sum() { curl -fsSL "$1" | sha256sum | cut -d' ' -f1; }

echo ":: source tarball v$version"
src_sum="$(sum "$repo/archive/v$version.tar.gz")"
sed -i "s/^pkgver=.*/pkgver=$version/; s/^sha256sums=.*/sha256sums=('$src_sum')/" PKGBUILD
cp PKGBUILD packaging/aur/omafile/PKGBUILD
packaging/aur/gen-srcinfo.sh packaging/aur/omafile/PKGBUILD

echo ":: binary tarball v$version"
bin_sum="$(sum "$repo/releases/download/v$version/omafile-$version-x86_64.tar.gz")"
sed -i "s/^pkgver=.*/pkgver=$version/; s/^sha256sums_x86_64=.*/sha256sums_x86_64=('$bin_sum')/" packaging/aur/omafile-bin/PKGBUILD
packaging/aur/gen-srcinfo.sh packaging/aur/omafile-bin/PKGBUILD
echo "done. Review with: git diff PKGBUILD packaging/aur ; then packaging/aur/publish.sh"
