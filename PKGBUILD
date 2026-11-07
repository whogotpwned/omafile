# Maintainer: whogotpwned
pkgname=omafile
pkgver=0.1.0
pkgrel=1
pkgdesc="Keyboard-driven terminal file manager that follows the Omarchy theme"
arch=('x86_64' 'aarch64')
url="https://github.com/whogotpwned/omafile"
license=('MIT')
depends=('gcc-libs' 'glibc')
optdepends=(
  'alacritty: terminal used by the desktop entry and the Hyprland binding'
  'glib2: move files to trash with gio'
  'wl-clipboard: copy paths to the Wayland clipboard'
  'xdg-utils: open non-text files with the default application'
  'ttf-cascadia-mono-nerd: file type icons'
  'neovim: default editor for text files'
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
  install -Dm644 contrib/omafile.desktop "$pkgdir/usr/share/applications/omafile.desktop"
  install -Dm644 contrib/shell-integration.sh "$pkgdir/usr/share/omafile/shell-integration.sh"
  install -Dm644 contrib/shell-integration.fish "$pkgdir/usr/share/omafile/of.fish"
  install -Dm644 contrib/hyprland-bindings.conf "$pkgdir/usr/share/omafile/hyprland-bindings.conf"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
}
