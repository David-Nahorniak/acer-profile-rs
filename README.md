# acer-profile

Řízení výkonnostních profilů pro **Acer Swift SFG14-73** (Intel Core Ultra 9
185H, Meteor Lake-H) na Linuxu. Jeden statický Rust binár, který nahrazuje
`power-profiles-daemon` (PPD) na DBus system bus a zároveň vlastní RAPL, EPP,
governor a i915 GPU frekvence.

## Proč to existuje

Na tomto stroji je `power-profiles-daemon` **rozbitý**: při přepnutí z
`performance` do jiného profilu vrací zápis EPP `EBUSY`, protože PPD píše EPP
dřív než governor, a `intel_pstate` v `governor=performance` vlastní EPP a
jakýkoli zápis vrací trvale `EBUSY`. Profil uvízne a Plasma slider přestane
fungovat.

`acer-profile` řeší problém přímo: vlastní DBus jméno `net.hadess.PowerProfiles`
(i alias `org.freedesktop.UPower.PowerProfiles` pro PowerDevil), aplikuje
správné pořadí zápisů (governor → EPP → RAPL → GPU) a při
`governor=performance` EPP přeskočí. Žádný prostředník, žádný EBUSY.

> **Legacy reference:** Neúplná původní verze v Pythonu je zachována v
> `legacy-python/` pouze jako referenční implementace (vyvíjena před přepisem
> do Rustu, nepoužívat v provozu). Rust verze je aktuální a plnohodnotná.

## Požadavky

### Build

| Požadavek | Verze | Poznámka |
|---|---|---|
| Rust toolchain (`rustc` + `cargo`) | stable (testováno 1.96.1) | `install.sh` doinstaluje přes pacman/apt/dnf nebo rustup fallback |
| `pkg-config` | libovolná | cargo občas vyžaduje při linkování |

### Runtime

| Požadavek | Balíček (Arch/Debian/Fedora) | Poznámka |
|---|---|---|
| Linux kernel | ≥ 6.x | sysfs `intel-rapl`, `cpufreq/policy*`, `i915` |
| `systemd` | — | DBus system bus + service unit |
| `dbus` (`busctl`) | `systemd` / `dbus` / `systemd dbus-tools` | pro ověření + PowerDevil komunikaci |
| `stress-ng` | `stress-ng` | subcommand `measure` a `probe-pl2` (zátěž CPU) |
| `lm_sensors` (`sensors`) | `lm_sensors` / `lm-sensors` / `lm_sensors` | čtení Package teploty (`sensors | grep Package`) |

### Kernel / hardware předpoklady

- **`intel_pstate`** driver aktivní (`scaling_governor`, `energy_performance_preference`).
- **`intel-rapl`** powercap (`/sys/class/powercap/intel-rapl:0/`).
- **`i915`** GPU driver (`gt_max_freq_mhz`, `gt_boost_freq_mhz` zapisovatelné).
  Na tomto stroji Intel Arc Graphics (Meteor Lake-P) používá kernel driver
  `i915` (ne `xe`).
- **`cpufreq` policies** na `/sys/devices/system/cpu/cpufreq/policy*` (22 policy).

Hardware specifikace Acer Swift SFG14-73 / Intel Core Ultra 9 185H (Meteor Lake-H):
- PBP=45 W (PL1), cTDP=35–65 W, MTP=115 W.
- Probe-validovaný PL2 strop=64 W (5089 MHz@83 °C; 80 W→98 °C throttle).
- i915 GPU: RPn/RP1=800 MHz, RP0/boost=2350 MHz.

> Na strojích bez těchto sysfs cest `acer-profile` gracefully přeskočí
> chybějící páky (nezhavaruje). Funkce ale závisí na jejich existenci.

## Co ovládá

| Pák | sysfs cesta | Poznámka |
|---|---|---|
| RAPL PL1 (long-term) | `intel-rapl:0/constraint_0_power_limit_uw` | Sustained power limit |
| RAPL PL2 (short-term) | `intel-rapl:0/constraint_1_power_limit_uw` | Turbo power limit |
| EPP | `policy*/energy_performance_preference` | `power` / `balance_power` / `performance` |
| Governor | `policy*/scaling_governor` | `powersave` / `performance` |
| GPU max freq | `i915/.../gt_max_freq_mhz` | i915 writable |
| GPU boost freq | `i915/.../gt_boost_freq_mhz` | i915 writable, max musí být ≥ boost |

Cesty se objevují za běhu (glob přes `std::fs`), chybějící páky se gracefully
přeskočí. **Fan control není dostupný** (EC neimplementuje WMI misc setting
0x000B) — ventila­tory zůstávají na EC auto-řízení.

## Profily

Hodnoty kalibrované měřením (viz `docs/PROFILY-TESTOVANI.md`):

| Profil | PL1 | PL2 | EPP | Governor | GPU max/boost |
|---|---|---|---|---|---|
| `eco` | 20 W | 25 W | `power` | `powersave` | 800 / 800 MHz |
| `normal` | 35 W | 45 W | `balance_power` | `powersave` | 1600 / 1600 MHz |
| `performance` | 45 W | 64 W | `performance` | `performance` | 2350 / 2350 MHz |

Alias názvy (pro pohodlí / DBus kompatibilitu):

| Alias | Kanonický |
|---|---|
| `power-saver` | `eco` |
| `balanced` | `normal` |
| `performance` / `perf` | `performance` |

Konfigurace: `/etc/acer-profile/profiles.toml` (TOML, viz `config/profiles.toml`).
Po úpravě: `sudo systemctl restart acer-profile`.

State file: `/var/lib/acer-profile/current` (jeden řádek = kanonický název).
Persistuje se napříč rebootem, daemon ho po startu aplikuje.

## Instalace

```bash
sudo ./install.sh
```

Skript:
1. Zkontroluje a doinstaluje chybějící závislosti (`rust`/`cargo`, `stress-ng`,
   `lm_sensors`, `dbus`) přes `pacman` / `apt` / `dnf` (fallback `rustup`).
2. `cargo build --release`.
3. Zastaví starý Python daemon (`systemctl stop acer-profile`) a smaže staré
   Python soubory (`acer_profile/` v site-packages, wrappery `/usr/bin/acer-profile`
   a `/usr/bin/acer-profiled`). **State + config se zachovají.**
4. Nainstaluje binár do `/usr/bin/acer-profile`.
5. Zazálohuje existující config (nepřepíše uživatelské hodnoty z probe).
6. `systemctl mask power-profiles-daemon` (zabrání kolizi o DBus jméno; balíček
   zůstává, nikdy nenaběhne).
7. Nainstaluje systemd unitu, `enable --now acer-profile.service`.
8. Ověří: `acer-profile status` + `busctl introspect`.

## Použití

```bash
# Stav hardwarových pák (čtení, bez roota)
acer-profile status

# Seznam profilů s hodnotami
acer-profile list

# Nastavení profilu (vyžaduje root — zápis sysfs + state file)
sudo acer-profile set eco
sudo acer-profile set normal
sudo acer-profile set performance

# Sledování změn profilu (foreground, bez roota)
acer-profile watch

# Srovnání profilů: zátěž + vzorkování CPU freq/teploty (root)
sudo acer-profile measure

# Detekce stropu PL2 s teplotní ochranou (root)
sudo acer-profile probe-pl2

# DBus service mód (pro systemd, normálně nespouštět ručně)
acer-profile daemon
```

### Plasma slider

Po instalaci a restartu `plasma-powerdevil.service` funguje Plasma battery
widget slider (Power Saver / Balanced / Performance) přes DBus do root daemonu
— **bez sudo, bez EBUSY**.

Pokud widget po instalaci ukazuje chybovou hlášku, restartuj PowerDevil:

```bash
systemctl --user restart plasma-powerdevil.service
```

Plasma používá dvě kontroly:
1. `isServiceRegistered("org.freedesktop.UPower.PowerProfiles")` na system bus
   → "PPD nainstalován".
2. `GetAll` na cestě `/org/freedesktop/UPower/PowerProfiles` → načte `Profiles`
   a `ActiveProfile`.

Proto daemon registruje **obě** bus jména (`net.hadess.PowerProfiles` +
`org.freedesktop.UPower.PowerProfiles`) a slouží interface na **obou** cestách
(`/net/hadess/PowerProfiles` + `/org/freedesktop/UPower/PowerProfiles`).

## Subcommandy detailně

### `status`

Vypíše: aktivní profil, RAPL PL1/PL2 (aktuální + max), EPP, governor,
CPU max frekvenci, GPU frekvence (cur/max/boost/RP0/RPn), teploty.

### `list`

Vypíše všechny 3 profily s hodnotami a aliasy.

### `set <profil>`

Aplikuje profil (governor → EPP → RAPL → GPU), persistuje do state file.
Akceptuje aliasy (`power-saver`, `balanced`, `performance`, `eco`, `normal`,
`perf`). **Vyžaduje root.**

### `measure`

Pro každý profil (`eco`, `normal`, `performance`):
1. `set` profil, 2 s settle.
2. Snapshot (idle): EPP/gov/PL1/PL2/GPU/teplota.
3. 15 s `stress-ng --cpu $(nproc)`, vzorkování `scaling_cur_freq` (max, průměr).
4. Snapshot po zátěži.
5. 20 s cooldown.

Na konci návrat na `normal`. Výstup je tabulka pro porovnání frekvencí a teplot
mezi profily. **Vyžaduje root.**

### `probe-pl2`

Bezpečná inkrementální detekce stropu PL2:
- Kroky: 64 → 80 → 100 → 115 W (nikdy nepřesáhne Intel MTP 115 W).
- 8 s `stress-ng` na každý krok, teplotní ochrana 95 °C (při překročení
  zastaví).
- 20 s cooldown mezi kroky.
- Detekuje VRM/EC tunel (když přijatý W ≠ požadovaný W).
- Na konci návrat na `normal` + doporučení pro `profiles.toml`.

**Vyžaduje root.**

### `watch`

Sleduje state file, při změně vypíše timestamp + profil + `status`. Default
interval 1 s (`-i`). Bez roota (jen čtení).

### `daemon`

DBus service mód — registruje `net.hadess.PowerProfiles` +
`org.freedesktop.UPower.PowerProfiles` na system bus, aplikuje startovní profil
z state file, polling (1 s) sleduje state file pro CLI změny a emituje
`PropertiesChanged` na obou cestách. Běží přes systemd (`Type=simple`).

## Architektura

```
acer-profile (jeden binár)
├── main.rs       clap dispatch: status/list/set/watch/measure/probe-pl2/daemon
├── levers.rs     sysfs: RAPL, EPP/governor (pořadí+EBUSY retry+skip), i915 GPU, teploty
├── profiles.rs   serde Profile + TOML load + aliasy + PPD name mapping
├── controller.rs apply pořadí + state file + set_profile
├── dbus.rs       net.hadess.PowerProfiles + org.freedesktop.UPower.PowerProfiles (alias)
├── daemon.rs     blocking DBus connection + boot persist + polling + PropertiesChanged
├── measure.rs    measure subcommand (stress-ng + sampling)
└── probe.rs      probe-pl2 subcommand (inkrementální + teplotní ochrana)
```

### Závislosti (Cargo)

```toml
serde, toml, clap, log, env_logger, anyhow, zbus (blocking, async-io feature), async-io
```

Žádný async runtime (tokio). Vše `std::*` + zbus blocking API.

### Klíčové implementční detaily

- **Pořadí zápisu**: governor → EPP → RAPL → GPU. Při `governor=performance`
  se EPP **přeskočí** (intel_pstate ho vlastní, jinak EBUSY).
- **EBUSY retry**: 3× se sleep 150 ms (pomáhá pro transientní race na
  eco/normal; pro `governor=performance` je EBUSY trvalý, proto skip).
- **GPU EINVAL tolerance**: i915 runtime PM race, errno 22 se toleruje (zápis
  se často přesto aplikuje).
- **glob bez externí crate**: vlastní `glob_paths()` přes `std::fs::read_dir`
  s `*` matchem.
- **DBus dual registration**: obě jména + obě cesty, aby PowerDevil proxy
  (`/org/freedesktop/UPower/PowerProfiles`) i `busctl` (`/net/hadess/...`)
  fungovaly.
- **PropertiesChanged**: vygenerovaný `<prop>_changed(&self, sc)` přes
  `async_io::block_on` z polling loopu (zbus 4.x).

## Získání zdrojového kódu / build

```bash
git clone https://github.com/David-Nahorniak/acer-profile-rs.git
cd acer-profile-rs

# Build
cargo build --release

# Lint
cargo clippy --release

# Binár
target/release/acer-profile
```

Předkompilované binárky (`.tar.gz`, `.deb`, `.rpm`) pro `x86_64` (gnu i musl)
jsou k dispozici na [GitHub Releases](https://github.com/David-Nahorniak/acer-profile-rs/releases).
Musl varianta je plně statický binár (bez glibc závislosti) — portable napříč
distry. GNU varianta + `.deb`/`.rpm` jsou linkované proti glibc (nativní pro
běžné distribuce).

Rust toolchain: stable (testováno 1.96.1). Pokud není nainstalovaný,
`install.sh` ho doinstaluje (pacman/apt/dnf nebo rustup fallback).

## DBus ověření

```bash
# Náš interface (net.hadess)
busctl introspect net.hadess.PowerProfiles /net/hadess/PowerProfiles

# Alias interface (PowerDevil používá tento)
busctl introspect org.freedesktop.UPower.PowerProfiles /org/freedesktop/UPower/PowerProfiles

# GetAll přes alias
busctl call org.freedesktop.UPower.PowerProfiles /org/freedesktop/UPower/PowerProfiles \
  org.freedesktop.DBus.Properties GetAll s org.freedesktop.UPower.PowerProfiles

# PowerDevil proxy (má vracet 3 profily)
busctl --user call org.kde.Solid.PowerManagement \
  /org/kde/Solid/PowerManagement/Actions/PowerProfile \
  org.kde.Solid.PowerManagement.Actions.PowerProfile profileChoices
```

## Reversibilita (návrat k PPD)

```bash
sudo systemctl disable --now acer-profile
sudo systemctl unmask power-profiles-daemon
sudo systemctl enable --now power-profiles-daemon
```

PPD byl jen **masknut** (ne odinstalován), takže návrat je reverzibilní.

## Bezpečnost

- RAPL = power limit, **ne napětí** (žádný overvolt). Nepersistuje přes reboot.
- `probe-pl2`: teplotní ochrana 95 °C, nikdy přes Intel MTP 115 W, inkrementální.
- THERMTRIP ~110 °C = hardwarová záchrana (OS neovládá).
- DBus service běží jako root (zápis sysfs); memory-safe Rust, attack surface
  omezený na DBus + sysfs (žádné vstupy z internetu).

## Soubory

```
acer-profile-rs/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── install.sh                     # instalace + mask PPD
├── PKGBUILD                       # AUR bonus (acer-profile-git)
├── acer-profile.install           # AUR pre/post hooks
├── config/profiles.toml           # kalibrované hodnoty profilů
├── systemd/acer-profile.service   # Type=simple, ExecStart=/usr/bin/acer-profile daemon
├── docs/PROFILY-TESTOVANI.md      # testovací dokumentace
├── src/
│   ├── main.rs
│   ├── levers.rs
│   ├── profiles.rs
│   ├── controller.rs
│   ├── dbus.rs
│   ├── daemon.rs
│   ├── measure.rs
│   └── probe.rs
└── legacy-python/                 # neúplná původní Python verze (jen reference)
    ├── acer_profile/
    ├── config/
    ├── docs/
    ├── systemd/
    ├── install.sh
    ├── phase1-*.sh / phase2-measure.sh / probe-pl2.sh
    └── pyproject.toml
```

## Licence

MIT.
