#!/bin/bash
# Fáze 1 revert: odstraní predator_v4=1, které na Swift SFG14-73 způsobuje
# broken /sys/firmware/acpi/platform_profile (WMI misc setting 0x000B nepodporováno).
# Fáze 2 (acer-profile daemon) nepotřebuje platform_profile.
# Spusť: sudo ./phase1-revert.sh
set -euo pipefail
CONF=/etc/modprobe.d/acer-wmi.conf

if [ -f "$CONF" ]; then
  cp -a "$CONF" "$CONF.revert.$(date +%s)"
  rm -f "$CONF"
  echo "Odstraněn $CONF (záloha *.revert.*)"
else
  echo "$CONF neexistuje - nic k revertu"
fi

if modprobe -r acer_wmi 2>/dev/null; then
  modprobe acer-wmi
  echo "acer-wmi reloadován bez predator_v4"
else
  echo "reload se nezdařil -> proveď reboot pro čistý stav"
fi

echo "--- platform_profile (mělo by zmizet / už neerrorovat) ---"
if [ -e /sys/firmware/acpi/platform_profile ]; then
  cat /sys/firmware/acpi/platform_profile 2>&1 || true
else
  echo "uzel neexistuje (OK - čistý intel_pstate režim)"
fi
echo "--- powerprofilesctl ---"
powerprofilesctl list
echo
echo "Revert hotový. powerprofilesctl nyní používá jen CpuDriver(intel_pstate)."
echo "Pro reálnou změnu výkonu aktivuj Fáze 2: sudo systemctl enable --now acer-profile"
