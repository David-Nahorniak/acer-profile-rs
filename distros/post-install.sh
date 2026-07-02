#!/bin/bash
# Post-install hook pro .deb/.rpm balíčky acer-profile.
# Ekvivalent acer-profile.install (AUR). Balíček sám instaluje soubory
# (binár, config, unit); zde jen systemd aktivace + mask PPD (DBus kolize).

set -e

# Mask power-profiles-daemon - nikdy nenaběhne, ani po reboot (reverzibilní).
if systemctl list-unit-files 2>/dev/null | grep -q 'power-profiles-daemon'; then
  systemctl disable --now power-profiles-daemon 2>/dev/null || true
  systemctl mask power-profiles-daemon
fi

systemctl daemon-reload
systemctl enable acer-profile.service

echo ":: acer-profile nainstalován."
echo "   Spusť: sudo systemctl start acer-profile"
echo "   PPD reverzibilita: sudo systemctl unmask power-profiles-daemon"
