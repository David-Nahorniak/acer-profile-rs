#!/bin/bash
# Fáze 1: ověření a měření profilů po predator_v4=1 (po rebootu)
# Spusť: ./phase1-verify.sh   (měření RAPL/fan vyžaduje root -> doporučeno sudo)
set -uo pipefail

SUDO=""
if [ "$(id -u)" -ne 0 ]; then
  SUDO="sudo"
fi

pp=/sys/firmware/acpi/platform_profile
echo "=== platform_profile ==="
if [ -f "$pp" ]; then
  echo "current : $(cat $pp)"
  echo "choices : $(cat /sys/firmware/acpi/platform_profile_choices 2>/dev/null)"
else
  echo "CHYBA: $pp neexistuje. predator_v4=1 asi nebyl aplikován -> zkontroluj /etc/modprobe.d/acer-wmi.conf a reboot."
  exit 1
fi

echo
echo "=== acer-wmi parametr ==="
cat /sys/module/acer_wmi/parameters/predator_v4 2>/dev/null || echo "(parametr nenalezen)"
journalctl -k 2>/dev/null | grep -iE "acer_wmi|acer-wmi" | tail -10

echo
echo "=== powerprofilesctl ==="
powerprofilesctl list

echo
echo "=== hwmon senzory (acer-wmi) ==="
$SUDO sensors 2>/dev/null | grep -iE "acer|fan|temp" || echo "(lm_sensors nenainstalován: sudo pacman -S lm_sensors)"
echo "--- acer-wmi hwmon cesty ---"
find /sys/devices/platform/acer-wmi -name "fan*_input" -o -name "temp*_input" 2>/dev/null | head
for f in $(find /sys/devices/platform/acer-wmi -name "fan*_input" 2>/dev/null) $(find /sys/devices/platform/acer-wmi -name "temp*_input" 2>/dev/null); do
  echo "$f = $(cat $f 2>/dev/null)"
done

echo
echo "=== RAPL limity ==="
for c in 0 1 2; do
  n=$(cat /sys/class/powercap/intel-rapl:0/constraint_${c}_name 2>/dev/null)
  pl=$(cat /sys/class/powercap/intel-rapl:0/constraint_${c}_power_limit_uw 2>/dev/null)
  mx=$(cat /sys/class/powercap/intel-rapl:0/constraint_${c}_max_power_uw 2>/dev/null)
  echo "c$c name=$n limit=${pl}uw max=${mx}uw"
done

echo
echo "=== EPP/governor ==="
echo "epp      : $(cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference)"
echo "governor : $(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor)"

snapshot() {
  local label="$1"
  echo "--- [$label] $(date -Is) ---"
  echo "pp       = $(cat $pp 2>/dev/null)"
  echo "ppc      = $(powerprofilesctl get 2>/dev/null)"
  echo "EPP      = $(cat /sys/devices/system/cpu/cpu0/cpufreq/energy_performance_preference)"
  echo "gov      = $(cat /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor)"
  echo "PL1      = $(cat /sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw)"
  echo "PL2      = $(cat /sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw)"
  for f in $(find /sys/devices/platform/acer-wmi -name "fan*_input" 2>/dev/null); do
    echo "$(basename $f) = $(cat $f)"
  done
  for f in $(find /sys/devices/platform/acer-wmi -name "temp*_input" 2>/dev/null); do
    echo "$(basename $f) = $(cat $f)"
  done
}

echo
echo "=== Srovnání profilů ==="
echo "Přepínám profily a měřím. (pro zátěž spusť v 2. terminálu: stress-ng --cpu \$(nproc) --timeout 30s)"
for prof in power-saver balanced performance; do
  echo
  echo ">>> powerprofilesctl set $prof"
  $SUDO powerprofilesctl set "$prof" 2>&1 || true
  sleep 2
  snapshot "$prof"
done

echo
echo "=== Návrat na balanced ==="
$SUDO powerprofilesctl set balanced 2>&1 || true
echo
echo "Hotovo. Vyhodnoť rozdíly PL1/PL2/EPP/fan mezi profily."
echo "Pokud jsou rozdíly malé nebo profily chybí -> přejít na Fázi 2 (Python daemon)."
