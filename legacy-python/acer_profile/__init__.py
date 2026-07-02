"""acer-profile: řízení výkonnostních profilů Acer Swift SFG14-73 na Linuxu.

Vrství se pod powerprofilesctl: naslouchá aktuálnímu profilu a aplikuje
doplňkové páky, které stock powerprofilesd neřeší:
  - RAPL PL1/PL2 power limity (hlavní zdroj reálné změny výkonu)
  - intel_pstate EPP + governor
  - i915 GPU frekvence (gt_max_freq / gt_boost_freq)
"""

__version__ = "0.1.0"
