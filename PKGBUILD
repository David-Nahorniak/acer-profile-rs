# Maintainer: David Nahorniak
# AUR bonus balíček: acer-profile-git (Rust verze, DBus PPD náhrada).
# Source: https://github.com/David-Nahorniak/acer-profile-rs

pkgname=acer-profile-git
pkgver=0.2.0
pkgrel=1
pkgdesc="Acer Swift SFG14-73 performance profile controller (RAPL+EPP+i915 GPU) + DBus power-profiles-daemon replacement"
arch=(x86_64)
url="https://github.com/David-Nahorniak/acer-profile-rs"
license=(MIT)
depends=(gcc-libs systemd stress-ng lm_sensors)
makedepends=(cargo git)
optdepends=('power-profiles-daemon: masknutý, nepoužívá se')
provides=(acer-profile power-profiles-daemon)
conflicts=(acer-profile power-profiles-daemon)
backup=(etc/acer-profile/profiles.toml)
install=acer-profile.install

_gitroot="https://github.com/David-Nahorniak/acer-profile-rs.git"
_gitname="acer-profile-rs"

build() {
  cd "$srcdir"
  msg "Připravuji zdroje..."
  if [ -d "$_gitname" ]; then
    cd "$_gitname" && git pull origin
    msg "Lokální adresář detekován - build z něj."
  else
    git clone "$_gitroot" "$_gitname" || true
    cd "$_gitname" 2>/dev/null || cd "$srcdir/$_gitname" 2>/dev/null || true
  fi

  # Fallback: pokud git clone selhal (lokální cesta bez repa), build přímo ze
  # zdrojovými soubory zkopírovanými do srcdir (pro lokální test AUR workflow).
  if [ ! -f Cargo.toml ]; then
    cp -a "$_gitroot"/{Cargo.toml,src,config,systemd} ./
  fi

  msg "Cargo build --release..."
  cargo build --release
}

package() {
  cd "$srcdir/$_gitname" 2>/dev/null || cd "$srcdir"

  install -Dm0755 target/release/acer-profile "$pkgdir/usr/bin/acer-profile"
  install -Dm0644 config/profiles.toml "$pkgdir/etc/acer-profile/profiles.toml"
  install -Dm0644 systemd/acer-profile.service "$pkgdir/usr/lib/systemd/system/acer-profile.service"
  install -Dm0644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE" 2>/dev/null || \
    install -Dm0644 /dev/null "$pkgdir/usr/share/licenses/$pkgname/LICENSE"

  install -d "$pkgdir/var/lib/acer-profile"
}

# vim:set ts=2 sw=2 et:
