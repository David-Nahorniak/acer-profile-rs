#!/bin/bash
# Instalace acer-profile (Rust) - jeden binár s DBus nahrazením PPD.
# Zakáže/maskne power-profiles-daemon (rozbitý EBUSY na Swift SFG14-73) a
# nainstaluje Rust binár, který vlastní net.hadess.PowerProfiles na system bus.
# Spusť: sudo ./install.sh
set -euo pipefail

SRC="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PKGDIR=/etc/acer-profile
UNITDIR=/etc/systemd/system
STATEDIR=/var/lib/acer-profile
BINDIR=/usr/bin
PY_SITE=$(/usr/bin/python -c "import sysconfig; print(sysconfig.get_paths()['purelib'])" 2>/dev/null || true)

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
#   rust/cargo  : build acer-profile
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

echo "[0/10] Kontrola závislostí (rust/cargo + stress-ng + sensors + dbus)"
if [ -z "$PKG_MGR" ]; then
  echo "  varování: nepodařilo se detekovat pacman/apt/dnf. Pokud něco chybí,"
  echo "  nainstaluj ručně: rust cargo stress-ng lm_sensors dbus"
fi
# cargo + rustc: nutné pro build. Pokud chybí, zkusíme rust balíček správce;
# fallbackem je rustup do uživatelského profilu (instaluje se do ~/.cargo/bin).
if ! command -v cargo >/dev/null 2>&1 || ! command -v rustc >/dev/null 2>&1; then
  echo "  chybí: cargo/rustc"
  # Arch/Debian/Fedora mají balíček 'rust' (+ 'cargo' na Debianu).
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
    # Přidat do PATH pro tento skript.
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
# Runtime závislosti subcommandů measure/probe-pl2 + teploty.
ensure_cmd stress-ng stress-ng stress-ng stress-ng
ensure_cmd sensors lm_sensors lm-sensors lm_sensors
# dbus (system bus) - obvykle už přítomen, ale ověřím.
if ! command -v busctl >/dev/null 2>&1; then
  echo "  chybí: busctl (systemd/dbus)"
  case "$PKG_MGR" in
    pacman) pkg_install systemd ;;
    apt)    pkg_install dbus systemd ;;
    dnf)    pkg_install systemd dbus-tools ;;
  esac
fi

# Znovu ověř klíčové nástroje po případné instalaci.
if ! command -v cargo >/dev/null 2>&1; then
  echo "  !! cargo stále chybí po instalaci. Oprav ručně a spusť install.sh znovu."
  exit 1
fi
echo "  závislosti OK (cargo $(cargo --version 2>/dev/null | awk '{print $2}'))"

echo "[1/10] Build (cargo build --release)"
cd "$SRC"
cargo build --release
BIN="$SRC/target/release/acer-profile"

echo "[2/10] Migrace: zastavení starého Python daemonu (pokud běží)"
if systemctl list-unit-files 2>/dev/null | grep -q '^acer-profile\.service'; then
  systemctl stop acer-profile.service 2>/dev/null || true
  systemctl disable acer-profile.service 2>/dev/null || true
  echo "  starý acer-profile.service zastaven/disablován"
fi

echo "[3/10] Migrace: odstranění staré Python instalace (state + config zachovány)"
# Staré Python soubory v site-packages
if [ -n "$PY_SITE" ] && [ -d "$PY_SITE/acer_profile" ]; then
  rm -rf "$PY_SITE/acer_profile"
  echo "  odstraněno $PY_SITE/acer_profile"
fi
# Staré shell wrappery (nový binár je jeden: /usr/bin/acer-profile)
for w in acer-profile acer-profiled; do
  if [ -f "$BINDIR/$w" ] && grep -q 'acer_profile' "$BINDIR/$w" 2>/dev/null; then
    rm -f "$BINDIR/$w"
    echo "  odstraněn starý wrapper $BINDIR/$w"
  fi
done

echo "[4/10] Instalace bináre do $BINDIR"
install -m 0755 "$BIN" "$BINDIR/acer-profile"
echo "  /usr/bin/acer-profile"

echo "[5/10] Konfigurace do $PKGDIR (záloha existujícího)"
install -d "$PKGDIR"
if [ -f "$PKGDIR/profiles.toml" ]; then
  cp -a "$PKGDIR/profiles.toml" "$PKGDIR/profiles.toml.bak.$(date +%s)"
  echo "  existující config zálohován (přeskočeno přepsání - zachovány uživatelské hodnoty)"
else
  install -m 0644 "$SRC/config/profiles.toml" "$PKGDIR/profiles.toml"
  echo "  konfig instalován $PKGDIR/profiles.toml"
fi

echo "[6/10] State dir $STATEDIR (zachován pokud existuje)"
install -d "$STATEDIR"

echo "[7/10] Mask power-profiles-daemon (zabrání kolizi o DBus jméno)"
# mask (ne jen disable) - balíček zůstane, ale nikdy nenaběhne, ani po reboot.
if systemctl list-unit-files 2>/dev/null | grep -q 'power-profiles-daemon'; then
  systemctl disable --now power-profiles-daemon 2>/dev/null || true
  systemctl mask power-profiles-daemon
  echo "  power-profiles-daemon masknutý (reverzibilní: systemctl unmask)"
else
  echo "  power-profiles-daemon není přítomen (nic k masknutí)"
fi

echo "[8/10] Systemd unit + aktivace"
install -m 0644 "$SRC/systemd/acer-profile.service" "$UNITDIR/acer-profile.service"
systemctl daemon-reload
systemctl enable --now acer-profile.service
sleep 2
systemctl --no-pager --full status acer-profile.service || true

echo "[9/10] Ověření"
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
