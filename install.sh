#!/bin/bash
# Instalace acer-profile (Rust) - jeden binár s DBus nahrazením PPD.
# Zakáže/maskne power-profiles-daemon (rozbitý EBUSY na Swift SFG14-73) a
# nainstaluje Rust binár, který vlastní net.hadess.PowerProfiles na system bus.
#
# Podporuje dva režimy (autodetekce):
#   1) Tarball z GitHub Release: vedle skriptu leží předsestavený strom
#      usr/bin/acer-profile + etc/acer-profile/ + usr/lib/systemd/system/.
#      Instaluje hotový binár, build toolchain není potřeba.
#   2) Git checkout: binár chybí -> fallback na `cargo build --release`.
#
# Spusť: sudo ./install.sh
set -euo pipefail

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKGDIR=/etc/acer-profile
UNITDIR=/etc/systemd/system
STATEDIR=/var/lib/acer-profile
BINDIR=/usr/bin

# -----------------------------------------------------------------------
# Detekce režimu: předsestavený tarball strom vs. git checkout (build).
# -----------------------------------------------------------------------
PREBUILT_BIN="$SRC/usr/bin/acer-profile"
PREBUILT_CFG="$SRC/etc/acer-profile/profiles.toml"
PREBUILT_UNIT="$SRC/usr/lib/systemd/system/acer-profile.service"

if [ -x "$PREBUILT_BIN" ]; then
  MODE="tarball"
  BIN_SRC="$PREBUILT_BIN"
  CFG_SRC="$PREBUILT_CFG"
  UNIT_SRC="$PREBUILT_UNIT"
else
  MODE="build"
  BIN_SRC=""
  CFG_SRC="$SRC/config/profiles.toml"
  UNIT_SRC="$SRC/systemd/acer-profile.service"
fi
echo "Režim instalace: $MODE"

# -----------------------------------------------------------------------
# Detekce správce balíčků (Arch pacman / Debian apt / Fedora dnf).
# -----------------------------------------------------------------------
detect_pkg_mgr() {
  if command -v pacman >/dev/null 2>&1; then echo pacman
  elif command -v apt-get >/dev/null 2>&1; then echo apt
  elif command -v dnf >/dev/null 2>&1; then echo dnf
  else echo ""
  fi
}
PKG_MGR="$(detect_pkg_mgr)"

# Mapa: název závislosti -> balíček pro každý správce.
#   rust/cargo  : build acer-profile (pouze v režimu build)
#   stress-ng   : subcommand measure / probe-pl2
#   lm_sensors  : čtení Package teploty (sensors)
#   dbus        : system bus pro daemon
#   pkgconf     : pkg-config (některé systémy ho nemají v base; cargo občas vyžaduje)
pkg_install() {
  # $1 =Arch $2 =Debian/Ubuntu $3 =Fedora
  local dep=""
  case "$PKG_MGR" in
    pacman) dep="$1" ;;
    apt)    dep="$2" ;;
    dnf)    dep="$3" ;;
    *)      echo "  !! Neznámý správce balíčků (n pacman/apt/dnf). Nainstaluj ručně: $1/$2/$3"; return 1 ;;
  esac
  if [ -z "$dep" ]; then return 0; fi
  echo "  instalace přes $PKG_MGR: $dep"
  case "$PKG_MGR" in
    pacman) pacman -S --noconfirm --needed $dep ;;
    apt)    apt-get update -y && apt-get install -y $dep ;;
    dnf)    dnf install -y $dep ;;
  esac
}

ensure_cmd() {
  # ensure_cmd <cmd> <arch> <debian> <fedora>
  local cmd="$1"; shift
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "  chybí: $cmd"
    pkg_install "$@"
  fi
}

# -----------------------------------------------------------------------
# Runtime závislosti (potřebné v obou režimech).
# -----------------------------------------------------------------------
echo "[0] Kontrola runtime závislostí (stress-ng + sensors + dbus)"
if [ -z "$PKG_MGR" ]; then
  echo "  varování: nepodařilo se detekovat pacman/apt/dnf. Pokud něco chybí,"
  echo "  nainstaluj ručně: stress-ng lm_sensors dbus"
fi
# stress-ng: subcommand measure / probe-pl2.
ensure_cmd stress-ng stress-ng stress-ng stress-ng
# lm_sensors: čtení Package teploty (sensors).
ensure_cmd sensors lm_sensors lm-sensors lm_sensors
# dbus (system bus) - obvykle už přítomen, ale ověřím přes busctl.
if ! command -v busctl >/dev/null 2>&1; then
  echo "  chybí: busctl (systemd/dbus)"
  case "$PKG_MGR" in
    pacman) pkg_install systemd ;;
    apt)    pkg_install dbus systemd ;;
    dnf)    pkg_install systemd dbus-tools ;;
  esac
fi

# -----------------------------------------------------------------------
# Build závislosti (pouze režim build).
# -----------------------------------------------------------------------
if [ "$MODE" = "build" ]; then
  echo "[0b] Režim build: kontrola cargo/rustc + pkg-config"
  # cargo + rustc: nutné pro build. Pokud chybí, zkusíme rust balíček správce;
  # fallbackem je rustup do uživatelského profilu (instaluje se do ~/.cargo/bin).
  if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
    echo "  chybí: cargo/rustc"
    if [ "$PKG_MGR" = pacman ]; then
      pkg_install rust
    elif [ "$PKG_MGR" = apt ]; then
      pkg_install cargo rustc
    elif [ "$PKG_MGR" = dnf ]; then
      pkg_install rust cargo
    fi
  fi
  # Stále chybí? Fallback: rustup do uživatelského profilu (bez sudo, pro root).
  if ! command -v cargo >/dev/null 2>&1; then
    echo "  správce balíčků nenainstaloval cargo -> fallback rustup (do ~/.cargo)"
    if command -v curl >/dev/null 2>&1; then
      curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh
      sh /tmp/rustup-init.sh -y --default-toolchain stable --profile minimal --no-modify-path
      export PATH="$HOME/.cargo/bin:$PATH"
      echo "  rustup nainstalován (cargo v $HOME/.cargo/bin)"
      echo "  POZNÁMKA: přidej 'export PATH=\$HOME/.cargo/bin:\$PATH' do /root/.bashrc"
      echo "            nebo instaluj rust přes správce balíčků pro persistenci napříč rebootem."
    else
      echo "  !! curl chybí - nelze použít rustup fallback. Nainstaluj rust/cargo ručně."
      exit 1
    fi
  fi
  # Knihovna pkg-config (cargo ji občas vyžaduje při linkování systémových knihoven).
  if ! command -v pkg-config >/dev/null 2>&1; then
    ensure_cmd pkg-config pkgconf pkg-config pkgconfig
  fi
  if ! command -v cargo >/dev/null 2>&1; then
    echo "  !! cargo stále chybí po instalaci. Oprav ručně a spusť install.sh znovu."
    exit 1
  fi
  echo "  build toolchain OK (cargo $(cargo --version 2>/dev/null | awk '{print $2}'))"
fi

# -----------------------------------------------------------------------
# Získání bináre: instalace z tarballu nebo cargo build.
# -----------------------------------------------------------------------
if [ "$MODE" = "tarball" ]; then
  echo "[1] Instaluji předsestavený binár z tarballu"
  BIN="$BIN_SRC"
else
  echo "[1] Build (cargo build --release)"
  cd "$SRC"
  cargo build --release
  BIN="$SRC/target/release/acer-profile"
fi
if [ ! -x "$BIN" ]; then
  echo "  !! binár nenalezen: $BIN"
  exit 1
fi

echo "[2] Instalace bináre do $BINDIR"
install -m 0755 "$BIN" "$BINDIR/acer-profile"
echo "  /usr/bin/acer-profile"

echo "[3] Konfigurace do $PKGDIR (záloha existujícího)"
install -d "$PKGDIR"
if [ -f "$PKGDIR/profiles.toml" ]; then
  cp -a "$PKGDIR/profiles.toml" "$PKGDIR/profiles.toml.bak.$(date +%s)"
  echo "  existující config zálohován (přeskočeno přepsání - zachovány uživatelské hodnoty)"
else
  install -m 0644 "$CFG_SRC" "$PKGDIR/profiles.toml"
  echo "  konfig instalován $PKGDIR/profiles.toml"
fi

echo "[4] State dir $STATEDIR (zachován pokud existuje)"
install -d "$STATEDIR"

echo "[5] Mask power-profiles-daemon (zabrání kolizi o DBus jméno)"
# mask (ne jen disable) - balíček zůstane, ale nikdy nenaběhne, ani po reboot.
if systemctl list-unit-files 2>/dev/null | grep -q 'power-profiles-daemon'; then
  systemctl disable --now power-profiles-daemon 2>/dev/null || true
  systemctl mask power-profiles-daemon
  echo "  power-profiles-daemon masknutý (reverzibilní: systemctl unmask)"
else
  echo "  power-profiles-daemon není přítomen (nic k masknutí)"
fi

echo "[6] Systemd unit + aktivace"
install -m 0644 "$UNIT_SRC" "$UNITDIR/acer-profile.service"
systemctl daemon-reload
systemctl enable --now acer-profile.service
sleep 2
systemctl --no-pager --full status acer-profile.service || true

echo "[7] Ověření"
acer-profile status
echo
echo "DBus introspekce (měla by ukázat náš interface):"
echo "  busctl introspect net.hadess.PowerProfiles /net/hadess/PowerProfiles"
echo
echo "Instalace hotová. Použití (powerprofilesctl už NEPOUŽÍVEJ):"
echo "  acer-profile set performance     # výkonnostní profil"
echo "  acer-profile set normal          # běžný profil"
echo "  acer-profile set eco             # úsporný profil"
echo "  acer-profile status              # stav pák"
echo "  acer-profile list                # profily"
echo "  acer-profile measure             # srovnání profilů (root)"
echo "  acer-profile probe-pl2           # detekce stropu PL2 (root)"
echo "  journalctl -u acer-profile -f    # log daemonu"
echo
echo "Návrat k PPD (reverzibilita):"
echo "  systemctl disable --now acer-profile && systemctl unmask power-profiles-daemon"
echo "  systemctl enable --now power-profiles-daemon"
