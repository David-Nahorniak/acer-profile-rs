"""acer-profile CLI: status / list / set / apply / watch.

Standalone: `acer-profile set <profil>` je primární rozhraní.
power-profiles-daemon se doporučuje zakázat (install.sh to udělá).
"""

from __future__ import annotations

import argparse
import logging
import sys
import time

from . import controller, levers
from .profiles import PROFILE_ALIASES, canonical, load_profiles, resolve
from .controller import VALID


def _fmt_uw(uw: int | None) -> str:
    return f"{uw / 1_000_000:.1f} W" if uw is not None else "-"


def cmd_status(_args) -> int:
    s = levers.status()
    cur = controller.current_profile()
    print(f"aktivní profil   : {cur or '(žádný)'}")
    rapl = s.get("rapl")
    if rapl:
        print(f"RAPL PL1 (long)  : {_fmt_uw(rapl['pl1_uw'])}  max {_fmt_uw(rapl['pl1_max_uw'])}")
        print(f"RAPL PL2 (short) : {_fmt_uw(rapl['pl2_uw'])}  max {_fmt_uw(rapl['pl2_max_uw'])}")
    print(f"EPP              : {s.get('epp')}")
    print(f"Governor         : {s.get('governor')}")
    freq = s.get("cpu_freq_khz")
    print(f"CPU max freq     : {freq/1000:.0f} MHz" if freq else "CPU max freq     : -")
    gpu = s.get("gpu") or {}
    if gpu:
        print(f"GPU freq         : cur {gpu.get('cur')} MHz, max {gpu.get('max')} MHz, "
              f"boost {gpu.get('boost')} MHz (RP0 {gpu.get('rp0')} / RPn {gpu.get('rpn')})")
    temps = s.get("temps") or {}
    if temps:
        tstr = ", ".join(f"{k}={v/1000:.0f}°C" for k, v in temps.items())
        print(f"Teploty          : {tstr}")
    return 0


def cmd_list(_args) -> int:
    profiles = load_profiles()
    for name in VALID:
        p = profiles.get(name)
        if not p:
            continue
        aliases = [k for k, v in PROFILE_ALIASES.items() if v == name and k != name]
        print(f"[{name}]  (aliasy: {', '.join(aliases) or '-'})")
        print(f"   PL1={_fmt_uw(p.pl1_uw)}  PL2={_fmt_uw(p.pl2_uw)}")
        print(f"   EPP={p.epp}  governor={p.governor}")
        print(f"   GPU max={p.gpu_max_mhz} boost={p.gpu_boost_mhz} MHz")
    return 0


def cmd_set(args) -> int:
    can = canonical(args.profile)
    if can is None or can not in VALID:
        print(f"Neznámý profil: {args.profile}", file=sys.stderr)
        print(f"Možnosti: {', '.join(VALID)} (aliasy: {', '.join(PROFILE_ALIASES)})", file=sys.stderr)
        return 2
    if not controller.set_profile(can):
        print(f"Chyba při aplikaci profilu {can}", file=sys.stderr)
        return 1
    print(f"Nastaven a aplikován profil: {can}")
    return 0


def cmd_watch(args) -> int:
    interval = args.interval
    last = controller.current_profile()
    try:
        while True:
            cur = controller.current_profile()
            if cur != last:
                print(f"[{time.strftime('%H:%M:%S')}] profil={cur}")
                last = cur
                cmd_status(args)
                print("-" * 60)
            time.sleep(interval)
    except KeyboardInterrupt:
        return 0


def build_parser() -> argparse.ArgumentParser:
    p = argparse.ArgumentParser(prog="acer-profile", description=__doc__)
    sub = p.add_subparsers(dest="cmd", required=True)
    sub.add_parser("status", help="aktuální stav hardwarových pák").set_defaults(func=cmd_status)
    sub.add_parser("list", help="seznam profilů").set_defaults(func=cmd_list)
    sp = sub.add_parser("set", help="nastavit a aplikovat profil (eco/normal/performance)")
    sp.add_argument("profile")
    sp.set_defaults(func=cmd_set)
    wp = sub.add_parser("watch", help="sledovat změny profilu (foreground)")
    wp.add_argument("-i", "--interval", type=float, default=1.0)
    wp.set_defaults(func=cmd_watch)
    return p


def main() -> int:
    logging.basicConfig(level=logging.WARNING, format="%(levelname)s %(message)s")
    args = build_parser().parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
