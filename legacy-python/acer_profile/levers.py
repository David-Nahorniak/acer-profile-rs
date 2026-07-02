"""Nízkoúrovňové páky (levers) pro řízení výkonu.

Každá funkce objeví cesty za běhu a gracefully přeskočí to, co na stroji
neexistuje nebo není zapisovatelné. Vše běží jako root (systemd service).

Klíčové: zápis governor PŘED EPP. Na intel_pstate vrací zápis EPP EBUSY,
pokud je scaling_governor=performance (governor vlastní policy). Proto
nejdřív governor=powersave, pak EPP.
"""

from __future__ import annotations

import glob
import logging
import os
import time
from dataclasses import dataclass

log = logging.getLogger(__name__)

RAPL_PKG_GLOB = "/sys/class/powercap/intel-rapl:*"
CPU_POLICY_GLOB = "/sys/devices/system/cpu/cpufreq/policy*"
I915_CARD_GLOB = "/sys/bus/pci/drivers/i915/*:*/drm/card*/gt_max_freq_mhz"


def _read(path: str) -> str | None:
    try:
        with open(path, "r", encoding="utf-8") as fh:
            return fh.read().strip()
    except OSError:
        return None


def _as_int(s: str | None) -> int | None:
    if s is None or s == "":
        return None
    try:
        return int(s)
    except ValueError:
        return None


def _write(path: str, value: str | int, retry_ebusy: bool = True) -> bool:
    val = str(value)
    for attempt in range(3 if retry_ebusy else 1):
        try:
            with open(path, "r+", encoding="utf-8") as fh:
                old = fh.read().strip()
                if old == val:
                    return True
                fh.seek(0)
                fh.write(val + "\n")
                fh.truncate()
            return True
        except PermissionError:
            log.warning("write denied (ro): %s", path)
            return False
        except OSError as exc:
            if exc.errno == 16 and retry_ebusy and attempt < 2:  # EBUSY
                time.sleep(0.15)
                continue
            log.warning("write failed %s: %s", path, exc)
            return False
    return False


# ---------------------------------------------------------------- RAPL
@dataclass
class RaplConstraints:
    pkg: str
    pl1_uw: int | None
    pl2_uw: int | None
    pl1_max_uw: int | None
    pl2_max_uw: int | None


def rapl_discover() -> str | None:
    pkgs = sorted(glob.glob(RAPL_PKG_GLOB))
    for p in pkgs:
        if _read(os.path.join(p, "name")) == "package-0":
            return p
    return pkgs[0] if pkgs else None


def rapl_read(pkg: str | None = None) -> RaplConstraints | None:
    pkg = pkg or rapl_discover()
    if not pkg:
        return None
    return RaplConstraints(
        pkg=pkg,
        pl1_uw=_as_int(_read(f"{pkg}/constraint_0_power_limit_uw")),
        pl2_uw=_as_int(_read(f"{pkg}/constraint_1_power_limit_uw")),
        pl1_max_uw=_as_int(_read(f"{pkg}/constraint_0_max_power_uw")),
        pl2_max_uw=_as_int(_read(f"{pkg}/constraint_1_max_power_uw")),
    )


def rapl_apply(pkg: str | None, pl1_uw: int | None, pl2_uw: int | None) -> None:
    pkg = pkg or rapl_discover()
    if not pkg:
        log.warning("RAPL package nenalezen, preskakuji")
        return
    if pl1_uw is not None:
        _write(f"{pkg}/constraint_0_power_limit_uw", pl1_uw)
    if pl2_uw is not None:
        _write(f"{pkg}/constraint_1_power_limit_uw", pl2_uw)


# ----------------------------------------------- EPP/governor (pořadí!)
def cpu_policies() -> list[str]:
    return sorted(glob.glob(CPU_POLICY_GLOB))


def epp_apply(epp: str | None, governor: str | None = None) -> None:
    """governor PŘED EPP - jinak EBUSY při governor=performance.

    Důležité: při governor=performance intel_pstate VLASTNÍ EPP (performance
    governor řídí frekvenci sám, EPP je bezvýznamné) a zápis EPP vrací EBUSY
    trvale. Proto EPP v tomto případě vůbec nezapisujeme.
    """
    pols = cpu_policies()
    if not pols:
        log.warning("žádné cpufreq policy, EPP preskakuji")
        return
    if governor:
        for p in pols:
            _write(f"{p}/scaling_governor", governor)
    if epp and governor != "performance":
        for p in pols:
            _write(f"{p}/energy_performance_preference", epp, retry_ebusy=True)
    elif epp and governor == "performance":
        log.debug("governor=performance -> EPP preskočeno (driver ho vlastní)")


def epp_read() -> tuple[str | None, str | None]:
    pols = cpu_policies()
    if not pols:
        return None, None
    return _read(f"{pols[0]}/energy_performance_preference"), _read(f"{pols[0]}/scaling_governor")


# ----------------------------------------------------------------- GPU
def i915_card_dir() -> str | None:
    for f in glob.glob(I915_CARD_GLOB):
        return os.path.dirname(f)
    return None


def gpu_read_bounds() -> dict[str, int | None]:
    d = i915_card_dir()
    if not d:
        return {}
    return {
        "rp0": _as_int(_read(f"{d}/gt_RP0_freq_mhz")),
        "rp1": _as_int(_read(f"{d}/gt_RP1_freq_mhz")),
        "rpn": _as_int(_read(f"{d}/gt_RPn_freq_mhz")),
        "max": _as_int(_read(f"{d}/gt_max_freq_mhz")),
        "boost": _as_int(_read(f"{d}/gt_boost_freq_mhz")),
        "cur": _as_int(_read(f"{d}/gt_cur_freq_mhz")),
    }


def gpu_apply(max_mhz: int | None, boost_mhz: int | None) -> None:
    d = i915_card_dir()
    if not d:
        log.warning("i915 card nenalezen, GPU preskakuji")
        return
    # max před boost (boost nesmí > max), EINVAL na i915 tolerujeme.
    if max_mhz is not None:
        _write_gpu_freq(f"{d}/gt_max_freq_mhz", max_mhz)
    if boost_mhz is not None:
        _write_gpu_freq(f"{d}/gt_boost_freq_mhz", boost_mhz)


def _write_gpu_freq(path: str, value: int) -> None:
    try:
        with open(path, "r+", encoding="utf-8") as fh:
            old = fh.read().strip()
            val = str(value)
            if old == val:
                return
            fh.seek(0)
            fh.write(val + "\n")
            fh.truncate()
    except PermissionError:
        log.warning("GPU write denied (ro): %s", path)
    except OSError as exc:
        if exc.errno == 22:  # EINVAL - i915 race, často se přesto aplikuje
            log.debug("GPU write EINVAL (tolerováno) %s=%s", path, value)
        else:
            log.warning("GPU write failed %s: %s", path, exc)


# ------------------------------------------------------------- teploty
def temps_read() -> dict[str, int]:
    out: dict[str, int] = {}
    for f in glob.glob("/sys/devices/platform/acer-wmi/hwmon/*/temp*_input"):
        out[f"acer_{os.path.basename(f)}"] = _as_int(_read(f)) or 0
    for f in glob.glob("/sys/class/hwmon/hwmon*/temp1_input"):
        name = _read(os.path.join(os.path.dirname(f), "name")) or "hwmon"
        t = _as_int(_read(f))
        if t is not None:
            out[f"{name}"] = t
    return out


def cpu_max_freq_khz() -> int | None:
    best = 0
    for p in cpu_policies():
        v = _as_int(_read(f"{p}/scaling_cur_freq"))
        if v and v > best:
            best = v
    return best or None


# --------------------------------------------------- kompletní status
def status() -> dict:
    epp, gov = epp_read()
    r = rapl_read()
    return {
        "rapl": None if not r else {
            "pl1_uw": r.pl1_uw,
            "pl2_uw": r.pl2_uw,
            "pl1_max_uw": r.pl1_max_uw,
            "pl2_max_uw": r.pl2_max_uw,
        },
        "epp": epp,
        "governor": gov,
        "gpu": gpu_read_bounds(),
        "temps": temps_read(),
        "cpu_freq_khz": cpu_max_freq_khz(),
    }
