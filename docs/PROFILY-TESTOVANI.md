# Profilové hodnoty — testování a kalibrace

Tento dokument popisuje testy provedené pro kalibraci výkonnostních profilů
`acer-profile` na Acer Swift SFG14-73 (Intel Core Ultra 9 185H) a zdůvodňuje
hodnoty v `config/profiles.toml`. Neobsahuje README k celému projektu.

## Stroj a CPU

- **Notebook:** Acer Swift SFG14-73, BIOS V1.21 (02/2026)
- **CPU:** Intel Core Ultra 9 185H (Meteor Lake-H)
  - CPUID model 170, family 6
  - P-core turbo 5.1 GHz, E-core turbo 3.8 GHz, LP-E 2.5 GHz
  - `cpuinfo_max_freq = 5100000` kHz (5.1 GHz) — potvrzeno v `/proc/cpuinfo`
- **GPU:** i915 (Xe-LPG / Arc), `gt_RP0_freq_mhz = 2350`, `gt_RPn = gt_RP1 = 800`
- **OS:** Arch Linux, kernel 7.0.14-arch1-1, intel_pstate (active mode)

## Oficiální Intel specifikace (185H, Meteor Lake-H)

Zdroj: Intel ARK / Wikipedie (tabulka převzatá z ARK).

| Parametr | Hodnota | Význam |
|----------|---------|--------|
| Processor Base Power (PBP) | 45 W | sustained long-term (PL1) |
| Configurable TDP (cTDP) | 35–65 W | OEM rozsah pro PL1 |
| Maximum Turbo Power (MTP) | 115 W | short-term burst (PL2) |
| P-core max turbo | 5.1 GHz | |
| GPU max freq | 2.35 GHz | |

## Provedené testy

### Test 1 — Fáze 1: acer-wmi `predator_v4=1` (platform_profile)

**Cíl:** ověřit, zda lze Acer EC termální profil aktivovat přes vestavěný
`acer-wmi` modul (WMI gaming GUID `7A4DDFE7…`, misc setting `0x000B`),
čímž by `powerprofilesctl` začal ovládat skutečný EC profil místo `placeholder`.

**Postup:** `/etc/modprobe.d/acer-wmi.conf` s `options acer-wmi predator_v4=1`,
reload modulu, kontrola `/sys/firmware/acpi/platform_profile`.

**Výsledek:** NEPROŠEL.
- `platform_profile: Failed to get profile for handler acer-wmi` (kernel log)
- `cat /sys/firmware/acpi/platform_profile` → I/O chyba
- Hwmon senzory (teploty) přes stejný gaming GUID fungují, ale WMI misc
  setting `0x000B` (termální profil) EC Swift SFG14-73 neimplementuje.
- Závěr: platform_profile cesta je pro tento model nepoužitelná → Fáze 2
  (vlastní userspace vrstva přes RAPL/EPP/GPU sysfs).

### Test 2 — power-profiles-daemon (PPD) EBUSY

**Cíl:** použít `powerprofilesctl` jako rozhraní a acer-profile jen jako
doplňkovou vrstvu.

**Výsledek:** PPD je na tomto stroji **rozbitý**.
- `powerprofilesctl set power-saver` / `balanced` →
  `Failed to activate CPU driver 'intel_pstate': Error writing
  '.../policy11/energy_performance_preference': Zařízení nebo zdroj jsou
  používány (EBUSY)`.
- Příčina: PPD píše EPP **dřív** než přepne governor z `performance`.
  intel_pstate vrací EBUSY, když `scaling_governor=performance` (driver
  v tomto režimu EPP vlastní).
- Důsledek: profil uvízne na `performance`, stroj se vařil 88–95 °C v klidu.
- Závěr: přejít na standalone režim — `acer-profile` vlastní VŠECHNY páky,
  PPD zakázán (`install.sh` provede `systemctl disable --now
  power-profiles-daemon`).

### Test 3 — EPP zápis pořadí (governor→EPP)

**Cíl:** vyřešit EBUSY při přepínání na `performance`.

**Postup:** zápis `scaling_governor` PŘED `energy_performance_preference`;
při `governor=performance` EPP vůbec nezapisovat (driver ho vlastní).

**Výsledek:** OK. eco/normal (powersave) EPP se zapisuje čistě (`power`,
`balance_power`); performance EPP se přeskočí — žádné EBUSY.

### Test 4 — Srovnání profilů (CPU freq + teplota pod zátěží)

**Postup:** `phase2-measure.sh` — pro každý profil `acer-profile set`,
3 s settle, 15 s `stress-ng --cpu $(nproc)`, vzorkování `scaling_cur_freq`
max, teplota `sensors` (Package). Mezi profily 20 s cooldown (odstranění
tepelného nasáknutí z předchozího profilu).

**Výsledek (finální běh po opravách):**

| Profil | PL1 | PL2 | EPP | Governor | CPU freq pod zátěží | Teplota |
|--------|-----|-----|-----|----------|---------------------|---------|
| eco | 20 W | 25 W | power | powersave | 2476 MHz | 62 °C |
| normal | 35 W | 45 W | balance_power | powersave | 3668 MHz | 69 °C |
| performance | 45 W | 64 W | (skip) | performance | 3869 MHz | 88 °C |

- Frekvence rostou eco→normal→performance (2476 → 3668 → 3869 MHz).
- Teplota roste (62 → 69 → 88 °C), žádný throttle (<110 °C kritický limit).
- eco 20 W je pod Intel cTDP minimum (35 W), ale běží stabilně.
- GPU frekvence: eco 800 / normal 1600 / performance 2350 MHz (RP0 max).

### Test 5 — Probe PL2 (bezpečnostní strop)

**Cíl:** zjistit, zda lze PL2 zvednout nad BIOS default 64 W k Intel MTP
115 W, a nalézt bezpečný strop pro chlazení Acer Swift.

**Postup:** `probe-pl2.sh` (bezpečná verze, MAXTEMP=95 °C) — inkrementální
kroky 64 → 80 → 100 → 115 W, 8 s zátěž + teplotní dohled, 20 s cooldown.

**Výsledek:**

| PL2 | Zápis přijat | Max CPU freq | Teplota | Verdikt |
|-----|--------------|--------------|---------|---------|
| 64 W | ano | 5089 MHz | 83 °C | ✅ bezpečné (téměř max 5.1 GHz) |
| 80 W | ano | 5089 MHz | 98 °C | ❌ throttle (>95 °C, rušeno) |

- RAPL **není zamčený** (`constraint_1_max_power_uw = 0`, žádný `locked`),
  takže PL2 zápis nad BIOS default projde.
- Při 64 W CPU turi na 5089 MHz (≈ max 5.1 GHz). Extra power nad 64 W
  nepřináší vyšší frekvenci — jen teplo.
- Při 80 W 98 °C → throttle. Swift chladič je limitující, ne čip.
- **Bezpečný PL2 strop = 64 W.** Intel MTP 115 W je pro hrubé gaming
  noteboocy; tenký Swift ho neutáhne.
- Závěr: `performance.pl2_uw = 64000000` (potvrzeno měřením, ne odhad).

## Bezpečnostní poznámky

- **THERMTRIP# (~110 °C):** hardwarový shutoff v silikonu, OS ho nemůže
  přejít. Poslední záchrana před poškozením.
- **VRM current/thermal limit:** napájecí obvody samy omezí proud při
  překročení — throttling, ne poškození.
- **RAPL = power limit, ne napětí:** Vcore řídí čip sám; neděláme
  overvolt/undervolt přes MSR (to by bylo skutečné riziko).
- **Nepersistuje přes reboot:** po restartu RAPL → BIOS default. Žádný brick.
- **eco 20 W mimo Intel cTDP (35 W min):** agresivní úspora, ale měřeno
  stabilní (2476 MHz). Při případné nestabilitě navýšit na 28 W.

## Odkaz na config

Hodnoty profilů jsou v `config/profiles.toml` (zkrácené info + odkaz sem).
Po úpravě: `sudo systemctl restart acer-profile`.
