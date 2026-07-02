#!/bin/bash
# Post-upgrade hook pro .deb/.rpm balíčky acer-profile.
# Po výměně bináre/unit reload a restart daemonu.

set -e

systemctl daemon-reload
systemctl restart acer-profile.service 2>/dev/null || true
