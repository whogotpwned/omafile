# Maintainer: whogotpwned
pkgname=omafile
pkgver=0.30.0
pkgrel=1
pkgdesc="Native GTK4/libadwaita file manager that follows the Omarchy theme"
arch=('x86_64' 'aarch64')
url="https://github.com/whogotpwned/omafile"
license=('MIT')
depends=('gtk4' 'libadwaita' 'vte4' 'glib2' 'hicolor-icon-theme')
optdepends=(
  'gvfs: trash, removable drives, SFTP servers, archive browsing'
  'gvfs-afc: iPhone and iPad over USB'
  'libimobiledevice: iPhone and iPad over USB'
  'gvfs-mtp: Android phones over USB'
  'mpv: internet radio and podcasts'
  'gocryptfs: encrypted folders'
  'git: status badges, stage, commit, branches'
  'diffutils: compare files and folders'
  'poppler: PDF thumbnails, previews, page turning, merge and split'
  'fd: fast recursive search'
  'ripgrep: full-text search'
  'bat: syntax highlighting in previews'
  'jq: formatted JSON in previews'
  'libarchive: archives, eBook covers and metadata (bsdtar)'
  'imagemagick: rotate, convert and resize images, font specimens, HEIC import'
  'libheif: HEIC to JPEG when importing iPhone photos'
  'ffmpegthumbnailer: video thumbnails'
  'ffmpeg: cover art, video tools, media facts'
  'wl-clipboard: copy the newest screenshot or recording (omafile --copy-capture)'
  'libpulse: Now Playing spectrum that follows the real sound (parec)'
  'gst-plugins-good: play MP3, FLAC, Ogg and common video in the preview'
  'gst-plugins-bad: more video codecs for the preview player'
  'gst-libav: H.264 / AAC playback in the preview'
  'webp-pixbuf-loader: WebP pictures: theme backgrounds in the atelier, previews'
  'qrencode: QR code for "Share on Local Network"'
  'calibre: convert eBooks, eBook viewer'
  'foliate: eBook reader'
  'docker: container volumes in the sidebar (or podman)'
  'uv: fast Python venv creation and installs from the Python menu'
  'xdg-desktop-portal-hyprland: "Open With" chooser'
  'uwsm: launch terminals and readers as session apps'
)
makedepends=('cargo')
checkdepends=('curl')
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
  install -Dm644 -t "$pkgdir/usr/share/omafile/plugin/plugin/i18n" plugin/i18n/*.json
  install -Dm644 CHANGELOG.md "$pkgdir/usr/share/doc/$pkgname/CHANGELOG.md"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
  install -Dm644 README.md "$pkgdir/usr/share/doc/$pkgname/README.md"
}
