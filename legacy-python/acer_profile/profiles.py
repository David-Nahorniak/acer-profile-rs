"""Definice profilů a načítání konfigurace (TOML).

Standalone režim: acer-profile vlastní VŠECHNY páky (RAPL + EPP + governor +
GPU), protože power-profiles-daemon je na Swift SFG14-73 rozbitý (EBUSY při
přepínání z performance). powerprofilesctl už NENÍ rozhraní.

Profily:
  eco          -> úsporný (20 W)
  normal       -> běžný   (35 W)
  performance  -> výkon   (45/64 W)
"""

from __future__ import annotations

import logging
import os
import tomllib
from dataclasses import dataclass

log = logging.getLogger(__name__)

CONFIG_PATH = "/etc/acer-profile/profiles.toml"

# akceptované názvy profilů (včetně powerprofilesctl aliasů pro pohodlí)
PROFILE_ALIASES = {
    "power-saver": "eco",
    "balanced": "normal",
    "performance": "performance",
    "eco": "eco",
    "normal": "normal",
    "perf": "performance",
}


@dataclass
class Profile:
    pl1_uw: int | None = None      # long_term sustained (W * 1_000_000)
    pl2_uw: int | None = None      # short_term turbo
    epp: str | None = None         # intel_pstate energy_performance_preference
    governor: str | None = None    # performance / powersave
    gpu_max_mhz: int | None = None # i915 gt_max_freq_mhz
    gpu_boost_mhz: int | None = None


# Defaulty kalibrované pro Swift SFG14-73 (Lunar Lake):
#   RAPL PL1 max hlášen 45 W; PL2 (short) bez max, BIOS default 64 W.
#   i915 GPU: RPn/RP1=800 MHz, RP0/boost=2350 MHz.
DEFAULTS: dict[str, Profile] = {
    "eco": Profile(
        pl1_uw=20_000_000,   # 20 W
        pl2_uw=25_000_000,   # 25 W
        epp="power",
        governor="powersave",
        gpu_max_mhz=800,
        gpu_boost_mhz=800,
    ),
    "normal": Profile(
        pl1_uw=35_000_000,   # 35 W
        pl2_uw=45_000_000,   # 45 W
        epp="balance_power",
        governor="powersave",
        gpu_max_mhz=1600,
        gpu_boost_mhz=1600,
    ),
    "performance": Profile(
        pl1_uw=45_000_000,   # 45 W (RAPL max sustained)
        pl2_uw=64_000_000,   # 64 W (BIOS turbo short-term)
        epp="performance",
        governor="performance",
        gpu_max_mhz=2350,
        gpu_boost_mhz=2350,
    ),
}


def _profile_from_dict(d: dict) -> Profile:
    return Profile(
        pl1_uw=d.get("pl1_uw"),
        pl2_uw=d.get("pl2_uw"),
        epp=d.get("epp"),
        governor=d.get("governor"),
        gpu_max_mhz=d.get("gpu_max_mhz"),
        gpu_boost_mhz=d.get("gpu_boost_mhz"),
    )


def load_profiles() -> dict[str, Profile]:
    profiles = {k: v for k, v in DEFAULTS.items()}
    if not os.path.exists(CONFIG_PATH):
        return profiles
    try:
        with open(CONFIG_PATH, "rb") as fh:
            cfg = tomllib.load(fh)
    except OSError as exc:
        log.error("nelze číst %s: %s", CONFIG_PATH, exc)
        return profiles
    except tomllib.TOMLDecodeError as exc:
        log.error("neplatný TOML %s: %s", CONFIG_PATH, exc)
        return profiles
    for name, section in cfg.items():
        if name not in profiles:
            log.warning("ignoruji neznámý profil %s v konfigu", name)
            continue
        profiles[name] = _profile_from_dict(section)
    log.info("profily načteny z %s", CONFIG_PATH)
    return profiles


def resolve(name: str, profiles: dict[str, Profile]) -> Profile | None:
    key = PROFILE_ALIASES.get(name, name)
    return profiles.get(key)


def canonical(name: str) -> str | None:
    return PROFILE_ALIASES.get(name, name) if name in PROFILE_ALIASES else None
