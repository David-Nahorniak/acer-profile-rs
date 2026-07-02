#!/bin/bash
# Fáze 1: aktivace acer-wmi platform_profile přes predator_v4=1
# Spusť: sudo ./phase1-setup.sh
set -euo pipefail

CONF=/etc/modprobe.d/acer-wmi.conf

echo "[1/6] Záloha existujícího conf (pokud je)"
if [ -f "$CONF" ]; then
  cp -a "$CONF" "$CONF.bak.$(date +%s)"
  echo "  zálohováno $CONF -> *.bak.*"
fi

echo "[2/6] Zápis $CONF"
cat > "$CONF" <<'EOF'
# Vynutí acer-wmi predator_v4 quirk -> registrace platform_profile
# přes WMI gaming GUID (EC termální profil). Viz plan pro Swift SFG14-73.
options acer-wmi predator_v4=1
EOF
cat "$CONF"

echo "[3/6] Pokus o reload acer-wmi (může selhat pokud je v použití -> reboot)"
if modprobe -r acer_wmi 2>/tmp/acer-rm.err; then
  modprobe acer-wmi
  echo "  reload OK"
else
  echo "  reload se nezdařil (očekáváno pokud backlight/hotkeys drží modul):"
  cat /tmp/acer-rm.err
  echo "  -> provedu reboot automaticky? [ne] Spusť ručně: sudo reboot"
fi

echo "[4/6] Stav modulu a parametru"
modinfo acer_wmi 2>/dev/null | grep -E "^filename|^parm:.*predator" || true
grep -rE "predator_v4" /sys/module/acer_wmi/parameters/ 2>/dev/null || echo "  (parametr ve sysfs nenalezen - možná modul nenačten s novým options, proveď reboot)"

echo "[5/6] platform_profile"
if [ -f /sys/firmware/acpi/platform_profile ]; then
  echo "  current: $(cat /sys/firmware/acpi/platform_profile)"
  echo "  choices: $(cat /sys/firmware/acpi/platform_profile_choices)"
else
  echo "  /sys/firmware/acpi/platform_profile STALE neexistuje -> proveď reboot a znovu spusť phase1-verify.sh"
fi

echo "[6/6] powerprofilesctl"
powerprofilesctl list

echo
echo "Hotovo. Pokud platform_profile neexistuje, rebootni a spusť: ./phase1-verify.sh"
