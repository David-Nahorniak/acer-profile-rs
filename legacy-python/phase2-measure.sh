#!/bin/bash
# Fáze 2: měření reálných rozdílů mezi profily (standalone acer-profile).
# Lunar Lake nemá RAPL energy counter -> měříme CPU frekvenci pod zátěží
# a teplotu (to je důkaz reálné změny chování profilu).
# Spusť: ./phase2-measure.sh   (vyžaduje sudo pro zápis sysfs + zátěž)
set -uo pipefail
SUDO=""; [ "$(id -u)" -ne 0 ] && SUDO="sudo"

snap() {
  local lbl="$1"
  echo "--- [$lbl] $(date -Is) ---"
  echo "profil   = $(acer-profile status 2>/dev/null | grep 'aktivní profil' | sed 's/.*: //')"
  echo "EPP      = $(cat /sys/devices/system/cpu/cpufreq/policy0/energy_performance_preference)"
  echo "gov      = $(cat /sys/devices/system/cpu/cpufreq/policy0/scaling_governor)"
  echo "PL1      = $(cat /sys/class/powercap/intel-rapl:0/constraint_0_power_limit_uw)"
  echo "PL2      = $(cat /sys/class/powercap/intel-rapl:0/constraint_1_power_limit_uw)"
  local gtd; gtd=$(dirname /sys/bus/pci/drivers/i915/*:*/drm/card*/gt_max_freq_mhz 2>/dev/null)
  [ -d "$gtd" ] && echo "GPU max  = $(cat $gtd/gt_max_freq_mhz 2>/dev/null) MHz, boost=$(cat $gtd/gt_boost_freq_mhz 2>/dev/null) MHz"
  echo "teplota  = $(sensors 2>/dev/null | grep -i 'Package' | grep -oE '[0-9]+\.[0-9]' | head -1)°C"
}

# vzorkuje max CPU frekvenci (kHz) během zátěže -> průměr v MHz
sample_freq() {
  local dur="$1" sum=0 n=0
  local end=$(( $(date +%s) + dur ))
  while [ "$(date +%s)" -lt "$end" ]; do
    local mx=0
    for f in /sys/devices/system/cpu/cpufreq/policy*/scaling_cur_freq; do
      v=$(cat "$f" 2>/dev/null)
      [ -n "$v" ] && [ "$v" -gt "$mx" ] && mx=$v
    done
    sum=$(( sum + mx )); n=$(( n + 1 ))
    sleep 0.5
  done
  [ "$n" -gt 0 ] && echo $(( sum / n / 1000 )) || echo 0
}

measure() {
  local prof="$1"
  echo
  echo ">>> $SUDO acer-profile set $prof"
  $SUDO acer-profile set "$prof"
  sleep 2
  snap "$prof (idle)"
  echo ">>> zátěž 15s (stress-ng CPU), vzorkuji CPU freq..."
  $SUDO stress-ng --cpu "$(nproc)" --timeout 15s >/dev/null 2>&1 &
  local spid=$!
  local avg; avg=$(sample_freq 14)
  wait $spid 2>/dev/null || true
  echo "  prům. max CPU freq pod zátěží: ${avg} MHz"
  sleep 1
  snap "$prof (po zátěži)"
  # cooldown ať další profil nezačíná tepelně nasáklý
  echo ">>> cooldown 20s před dalším profilem..."
  sleep 20
}

echo "=== daemon ==="
systemctl is-active acer-profile 2>/dev/null && echo "daemon aktivní" || echo "daemon NEaktivní"

for p in eco normal performance; do
  measure "$p"
done

echo
echo ">>> návrat na normal"
$SUDO acer-profile set normal
echo
echo "Hotovo. Porovnej prům. CPU freq a PL1/PL2 mezi profily:"
echo "  eco (~20W, nízká frekv) < normal (~35W) < performance (~45W+, vyšší frekv)."
