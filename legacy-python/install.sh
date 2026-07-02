#!/bin/bash
# Instalace acer-profile (Fáze 2, standalone) - bez pip.
# Zakáže power-profiles-daemon (rozbitý EBUSY na Swift SFG14-73).
# Spusť: sudo ./install.sh
set -euo pipefail

SRC=/home/dn/Stažené/acer/acer-profile
PKGDIR=/etc/acer-profile
UNITDIR=/etc/systemd/system
STATEDIR=/var/lib/acer-profile
SITE=$(/usr/bin/python -c "import sysconfig; print(sysconfig.get_paths()['purelib'])")
PYBIN=$(/usr/bin/python -c "import sys; print(sys.executable)")
BINDIR=/usr/bin

echo "[1/7] Instalace Python balíčku do $SITE"
install -d "$SITE/acer_profile"
for f in __init__.py levers.py profiles.py controller.py daemon.py cli.py; do
  install -m 0644 "$SRC/acer_profile/$f" "$SITE/acer_profile/$f"
done
echo "  nainstalováno acer_profile ($PYBIN)"

echo "[2/7] Wrapper skripty do $BINDIR"
cat > "$BINDIR/acer-profile" <<EOF
#!/bin/sh
exec $PYBIN -m acer_profile.cli "\$@"
EOF
cat > "$BINDIR/acer-profiled" <<EOF
#!/bin/sh
exec $PYBIN -m acer_profile.daemon "\$@"
EOF
chmod 0755 "$BINDIR/acer-profile" "$BINDIR/acer-profiled"
echo "  acer-profile, acer-profiled"

echo "[3/7] Konfigurace do $PKGDIR"
install -d "$PKGDIR"
if [ -f "$PKGDIR/profiles.toml" ]; then
  cp -a "$PKGDIR/profiles.toml" "$PKGDIR/profiles.toml.bak.$(date +%s)"
fi
install -m 0644 "$SRC/config/profiles.toml" "$PKGDIR/profiles.toml"
echo "  konfig v $PKGDIR/profiles.toml"

echo "[4/7] State dir $STATEDIR"
install -d "$STATEDIR"

echo "[5/7] Zakázání power-profiles-daemon (rozbitý EBUSY)"
if systemctl is-active power-profiles-daemon >/dev/null 2>&1; then
  systemctl disable --now power-profiles-daemon
  echo "  power-profiles-daemon disabled"
else
  systemctl disable power-profiles-daemon 2>/dev/null || true
  echo "  power-profiles-daemon already inactive"
fi

echo "[6/7] Systemd unit + aktivace"
install -m 0644 "$SRC/systemd/acer-profile.service" "$UNITDIR/acer-profile.service"
systemctl daemon-reload
systemctl enable acer-profile.service
systemctl restart acer-profile.service
sleep 2
systemctl --no-pager --full status acer-profile.service || true

echo "[7/7] Ověření"
acer-profile status

echo
echo "Instalace hotová. Použití (powerprofilesctl už NEPOUŽÍVEJ):"
echo "  acer-profile set performance     # přepne na výkonnostní profil"
echo "  acer-profile set normal          # běžný profil"
echo "  acer-profile set eco             # úsporný profil"
echo "  acer-profile status              # stav pák"
echo "  acer-profile list                # profily"
echo "  journalctl -u acer-profile -f    # log daemonu"
