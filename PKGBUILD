# Maintainer: whogotpwned
pkgname=omafile
pkgver=0.3.0
pkgrel=1
pkgdesc="Native GTK4/libadwaita file manager that follows the Omarchy theme"
arch=('x86_64' 'aarch64')
url="https://github.com/whogotpwned/omafile"
license=('MIT')
depends=('gtk4' 'libadwaita' 'glib2' 'hicolor-icon-theme')
optdepends=(
  'gvfs: trash, removable drives and network locations'
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
  install -Dm644 data/hyprland-bindings.conf "$pkgdir/usr/share/omafile/hyprland-bindings.conf"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
}
