# Maintainer: whogotpwned
pkgname=omafile
pkgver=0.5.0
pkgrel=1
pkgdesc="Native GTK4/libadwaita file manager that follows the Omarchy theme"
arch=('x86_64' 'aarch64')
url="https://github.com/whogotpwned/omafile"
license=('MIT')
depends=('gtk4' 'libadwaita' 'vte4' 'glib2' 'hicolor-icon-theme')
optdepends=(
  'gvfs: trash, removable drives and SFTP servers'
  'gocryptfs: encrypted folders'
  'git: status badges in repositories'
  'poppler: PDF thumbnails and previews'
  'fd: fast recursive search'
  'libarchive: extract and create archives'
  'xdg-desktop-portal-hyprland: "Open With" chooser'
  'alacritty: terminal for "Open Terminal Here"'
  'uwsm: launch the terminal as a proper session app'
)
makedepends=('cargo')
source=("$pkgname-$pkgver.tar.gz::$url/archive/v$pkgver.tar.gz")
sha256sums=('SKIP')

prepare() {
  cd "$pkgname-$pkgver"
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  cd "$pkgname-$pkgver"
  export CARGO_TARGET_DIR=target
  cargo build --frozen --release
}

check() {
  cd "$pkgname-$pkgver"
  cargo test --frozen --release
}

package() {
  cd "$pkgname-$pkgver"
  install -Dm755 target/release/omafile "$pkgdir/usr/bin/omafile"
  install -Dm644 data/io.github.whogotpwned.Omafile.desktop \
    "$pkgdir/usr/share/applications/io.github.whogotpwned.Omafile.desktop"
  install -Dm644 data/icons/io.github.whogotpwned.Omafile.svg \
    "$pkgdir/usr/share/icons/hicolor/scalable/apps/io.github.whogotpwned.Omafile.svg"
  install -Dm644 data/icons/io.github.whogotpwned.Omafile-symbolic.svg \
    "$pkgdir/usr/share/icons/hicolor/symbolic/apps/io.github.whogotpwned.Omafile-symbolic.svg"
  install -Dm644 data/hyprland-bindings.lua "$pkgdir/usr/share/omafile/hyprland-bindings.lua"
  install -Dm644 data/hyprland-bindings.conf "$pkgdir/usr/share/omafile/hyprland-bindings.conf"
  install -Dm644 manifest.json "$pkgdir/usr/share/omafile/plugin/manifest.json"
  install -Dm644 plugin/BarWidget.qml "$pkgdir/usr/share/omafile/plugin/plugin/BarWidget.qml"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
}
