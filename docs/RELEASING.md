# Releasing omafile

The moving parts: a version in four files, a git tag, a GitHub Release with a
prebuilt tarball, two AUR packages, and the Omarchy plugin marketplace.

## Once

1. **Default branch.** GitHub › Settings › Branches: make `main` the default.
2. **Actions.** Nothing to configure: `ci.yml` runs on every push and pull
   request, `release.yml` on every `vX.Y.Z` tag, both with the repository's
   own token.
3. **AUR account.** Register at <https://aur.archlinux.org/register>, add your
   SSH public key under *My Account*. Publishing a new package is just the
   first `git push` to `ssh://aur@aur.archlinux.org/<name>.git`.
4. **Real-machine pass.** Work through [acceptance-checklist.md](acceptance-checklist.md)
   on an Omarchy install before the first tag.

## Every release

```sh
scripts/bump-version.sh 0.18.0      # Cargo.toml, Cargo.lock, PKGBUILDs, manifest.json, README badge, changelog stub
$EDITOR CHANGELOG.md                # write the entry
cargo test --release && cargo clippy --release && python scripts/check-i18n.py && bash plugin/validate.sh
git commit -am "Release 0.18.0"
git tag v0.18.0
git push origin main v0.18.0
```

The tag starts **Release** on GitHub Actions: it checks that the tag matches
the version files, builds in an Arch container, runs the tests, packs
`omafile-0.18.0-x86_64.tar.gz` (+ `.sha256`) with `scripts/make-dist.sh` and
publishes a GitHub Release whose notes are the changelog entry.

When the release is up:

```sh
packaging/aur/update.sh             # fills in both checksums, refreshes .SRCINFO
git commit -am "AUR checksums for 0.18.0" && git push origin main
packaging/aur/publish.sh            # pushes omafile and omafile-bin to the AUR
```

Users then install with `yay -S omafile-bin` (prebuilt) or `yay -S omafile`
(builds from the tag). The bar widget stays a separate
`omarchy plugin add https://github.com/whogotpwned/omafile.git --enable`.

## Marketplace

After the first tagged release, submit the plugin as described in
[marketplace-submission.md](marketplace-submission.md); later versions go
through the marketplace's verify form with the release commit SHA.

## What CI checks

| Job | Checks |
|---|---|
| `build` | `cargo build / test / clippy -D warnings` on Arch, translation catalogs, plugin manifest and version agreement, desktop entry, a start under Xvfb, the release tarball as an artifact |
| `package` | `makepkg` with the AUR PKGBUILD against the commit, installs the package, `namcap`, `.SRCINFO` freshness |

## Files

| Path | Role |
|---|---|
| `PKGBUILD` = `packaging/aur/omafile/PKGBUILD` | source package (AUR `omafile`); `plugin/validate.sh` keeps the two identical |
| `packaging/aur/omafile-bin/PKGBUILD` | prebuilt package from the GitHub Release tarball |
| `packaging/aur/gen-srcinfo.sh` | `.SRCINFO` via `makepkg`, or a built-in generator off Arch |
| `packaging/aur/update.sh`, `publish.sh` | checksums after a release, push to the AUR |
| `scripts/make-dist.sh` | the release tarball (`bin/`, `share/` = `/usr`) |
| `scripts/bump-version.sh` | one version everywhere |
| `scripts/check-i18n.py` | catalogs complete, placeholders intact |
