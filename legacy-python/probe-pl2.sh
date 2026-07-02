#!/bin/bash
# probe-pl2.sh - BEZPEČNÁ verze: inkrementální test stropu PL2 s teplotní ochranou.
#
# Bezpečnostní principy:
#   1. Nikdy nepřesáhne Intel MTP (115W) - to je spec čipu, ne overclock.
#   2. Inkrementální kroky (64->80->100->115), ne skok na maximum.
#   3. Krátké zátěže (8s) s teplotním limitem: pokud Package >95°C, ZASTAVÍ.
#   4. Mezi pokusy 20s cooldown ať se stroj nevypeče.
#   5. Po skriptu návrat na normal profil.
#
# Spusť: sudo ./probe-pl2.sh
set -uo pipefail

PKG=/sys/class/powercap/intel-rapl:0
MAXTEMP=95   # °C - při překročení ZASTAVÍM probe (ochrana proti throttlingu)
STEPS="64 80 100 115"

read_temp() {
  sensors 2>/dev/null | grep -i 'Package' | grep -oE '[0-9]+\.[0-9]' | head -1 | cut -d. -f1
}

read_pl1() { cat "$PKG/constraint_0_power_limit_uw"; }
read_pl2() { cat "$PKG/constraint_1_power_limit_uw"; }
write_pl2() { echo "$1" > "$PKG/constraint_1_power_limit_uw"; }

echo "=== probe-pl2.sh (bezpečná verze, MAXTEMP=${MAXTEMP}°C) ==="
echo "Intel 185H MTP=115W. Testuji inkrementálně: $STEPS"
echo

echo "[0] baseline: performance profil"
acer-profile set performance
echo "    PL1=$(read_pl1) PL2=$(read_pl2) teplota=$(read_temp)°C"
echo

best_w=64   # bezpečný start (BIOS default)
for w in $STEPS; do
  t=$(read_temp)
  if [ -n "$t" ] && [ "$t" -gt "$MAXTEMP" ]; then
    echo "!!! teplota ${t}°C > ${MAXTEMP}°C - ZASTAVUJI probe (ochrana)"
    break
  fi

  echo ">>> test PL2=${w}W (předchozí OK: ${best_w}W)"
  write_pl2 "$(( w * 1000000 ))"
  got=$(read_pl2)
  got_w=$(( got / 1000000 ))

  if [ "$got_w" -ne "$w" ]; then
    echo "    VRM/EC tunul zápis: požadováno ${w}W, přijato ${got_w}W"
    echo "    -> reálný strop je ~${got_w}W. Končím."
    break
  fi
  echo "    zápis přijat: ${got_w}W"

  echo "    zátěž 8s + teplotní dohled..."
  stress-ng --cpu "$(nproc)" --timeout 8s >/dev/null 2>&1 &
  spid=$!
  mx=0
  end=$(( $(date +%s) + 7 ))
  overheat=0
  while [ "$(date +%s)" -lt "$end" ]; do
    for f in /sys/devices/system/cpu/cpufreq/policy*/scaling_cur_freq; do
      v=$(cat "$f" 2>/dev/null); [ -n "$v" ] && [ "$v" -gt "$mx" ] && mx=$v
    done
    ct=$(read_temp)
    if [ -n "$ct" ] && [ "$ct" -gt "$MAXTEMP" ]; then
      overheat=1
      echo "    !!! ${ct}°C > ${MAXTEMP}°C pod zátěží - ruším zátěž"
      kill $spid 2>/dev/null || true
      break
    fi
    sleep 0.5
  done
  wait $spid 2>/dev/null || true
  ct=$(read_temp)
  echo "    max freq=$(( mx / 1000 ))MHz  teplota=${ct}°C"

  if [ "$overheat" = "1" ]; then
    echo "    -> ${w}W přehřívá. Bezpečný strop = ${best_w}W."
    break
  fi
  best_w=$w
  echo "    OK, cooldown 20s..."
  sleep 20
done

echo
echo "=== výsledek ==="
echo "Bezpečný PL2 (žádný throttle <${MAXTEMP}°C): ${best_w}W"
echo
echo "=== návrat na normal ==="
acer-profile set normal
echo "PL1=$(read_pl1) PL2=$(read_pl2)"
echo
echo "Doporučení: nastav v /etc/acer-profile/profiles.toml:"
echo "  [performance]"
echo "  pl2_uw = $(( best_w * 1000000 ))"
echo "Pak: sudo systemctl restart acer-profile"
