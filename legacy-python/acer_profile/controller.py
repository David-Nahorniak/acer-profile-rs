"""Aplikace profilu na hardwarové páky + správa stavu (state file).

Standalone režim: acer-profile je primární rozhraní (power-profiles-daemon
je na tomto stroji rozbitý). Aktuální profil se persistuje do
/var/lib/acer-profile/current, aby ho daemon aplikoval po startu a aby
CLI/daemon zůstali synchronizovaní.

Pořadí zápisu (důležité pro intel_pstate - jinak EBUSY):
  1. governor (powersave/performance)
  2. EPP
  3. RAPL PL1/PL2
  4. GPU max/boost
"""

from __future__ import annotations

import logging
import os

from . import levers
from .profiles import Profile, canonical, load_profiles, resolve

log = logging.getLogger(__name__)

STATE_DIR = "/var/lib/acer-profile"
STATE_FILE = f"{STATE_DIR}/current"
VALID = ("eco", "normal", "performance")


def current_profile() -> str | None:
    try:
        with open(STATE_FILE, "r", encoding="utf-8") as fh:
            return fh.read().strip() or None
    except OSError:
        return None


def save_profile(name: str) -> None:
    os.makedirs(STATE_DIR, exist_ok=True)
    with open(STATE_FILE, "w", encoding="utf-8") as fh:
        fh.write(name + "\n")


def apply_profile(profile: Profile, label: str) -> None:
    log.info("aplikuji profil %s", label)
    # 1. governor PŘED EPP (intel_pstate EBUSY fix)
    levers.epp_apply(profile.epp, profile.governor)
    # 2. RAPL
    levers.rapl_apply(None, profile.pl1_uw, profile.pl2_uw)
    # 3. GPU
    levers.gpu_apply(profile.gpu_max_mhz, profile.gpu_boost_mhz)


def set_profile(name: str) -> bool:
    """Najde profil, aplikuje ho, persistuje stav. True pokud OK."""
    can = canonical(name)
    if can is None or can not in VALID:
        log.warning("neznámý profil: %s", name)
        return False
    profiles = load_profiles()
    prof = resolve(can, profiles)
    if prof is None:
        log.warning("profil %s nenalezen v konfigu", can)
        return False
    apply_profile(prof, can)
    save_profile(can)
    return True
