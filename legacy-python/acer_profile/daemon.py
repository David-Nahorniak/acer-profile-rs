"""Daemon: aplikuje uložený profil při startu a sleduje změny state file.

Standalone režim - nespoléhá na power-profiles-daemon. Sleduje
/var/lib/acer-profile/current (polling). Když CLI `acer-profile set` změní
state file, daemon profil aplikuje.

Běží jako systemd service (root).
"""

from __future__ import annotations

import logging
import os
import signal
import sys
import time

from . import controller
from .profiles import load_profiles

log = logging.getLogger("acer-profiled")

POLL_SEK = 1.0
_running = True


def _stop(*_):
    global _running
    _running = False
    log.info("ukončuji daemon")


def main() -> int:
    logging.basicConfig(
        level=logging.INFO,
        format="%(asctime)s %(name)s %(levelname)s %(message)s",
    )
    signal.signal(signal.SIGTERM, _stop)
    signal.signal(signal.SIGINT, _stop)

    load_profiles()
    log.info("acer-profiled start; poll=%ss (standalone, bez power-profiles-daemon)", POLL_SEK)

    last: str | None = controller.current_profile()
    if last:
        log.info("startovní profil z state: %s", last)
        controller.set_profile(last)
    else:
        # žádný uložený stav -> bezpečný default (normal), ať stroj nestračí
        # na performance (horký) po instalaci.
        log.info("žádný state file -> startuji na normal")
        controller.set_profile("normal")
        last = "normal"

    while _running:
        time.sleep(POLL_SEK)
        cur = controller.current_profile()
        if cur is None:
            continue
        if cur != last:
            log.info("změna profilu %s -> %s", last, cur)
            controller.set_profile(cur)
            last = cur

    log.info("daemon zastaven")
    return 0


if __name__ == "__main__":
    sys.exit(main())
