#!/usr/bin/env bash
# Set the version everywhere it lives and open a changelog entry.
#   scripts/bump-version.sh 0.18.0
set -euo pipefail
new="${1:?usage: scripts/bump-version.sh X.Y.Z}"
[[ $new =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "version must look like 1.2.3" >&2; exit 2; }
here="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$here"
old="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
[ "$old" != "$new" ] || { echo "already at $new"; exit 0; }
sed -i "0,/^version = \"$old\"/s//version = \"$new\"/" Cargo.toml
sed -i "s/^pkgver=.*/pkgver=$new/; s/^pkgrel=.*/pkgrel=1/" PKGBUILD packaging/aur/omafile-bin/PKGBUILD
sed -i "s/^sha256sums=.*/sha256sums=('SKIP')/" PKGBUILD
sed -i "s/^sha256sums_x86_64=.*/sha256sums_x86_64=('SKIP')/" packaging/aur/omafile-bin/PKGBUILD
cp PKGBUILD packaging/aur/omafile/PKGBUILD
packaging/aur/gen-srcinfo.sh packaging/aur/omafile/PKGBUILD >/dev/null
packaging/aur/gen-srcinfo.sh packaging/aur/omafile-bin/PKGBUILD >/dev/null
sed -i "s/\"version\": \"$old\"/\"version\": \"$new\"/" manifest.json
sed -i "s/version-$old-/version-$new-/; s/Current version: \*\*$old\*\*/Current version: **$new**/" README.md
today="$(date +%Y-%m-%d)"
if grep -q "^## Unreleased" CHANGELOG.md; then
    # What was collected under "Unreleased" becomes this release.
    sed -i "0,/^## Unreleased.*/s//## $new — $today/" CHANGELOG.md
elif ! grep -q "^## $new " CHANGELOG.md; then
    awk -v v="$new" -v d="$today" '!done && /^## [0-9]/ { print "## " v " — " d "\n\n### Changed\n- \n"; done=1 } { print }' CHANGELOG.md > CHANGELOG.md.new && mv CHANGELOG.md.new CHANGELOG.md
fi
cargo update -p omafile --offline >/dev/null 2>&1 || cargo generate-lockfile --offline >/dev/null 2>&1 || true
echo "bumped $old -> $new. Fill in CHANGELOG.md, then: git commit -am \"Release $new\" && git tag v$new && git push origin main v$new"
