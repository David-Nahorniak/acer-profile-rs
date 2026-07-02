#!/bin/bash
# Pre-remove hook pro .deb/.rpm balíčky acer-profile.
# Zastaví daemon před odstraněním souborů.

set -e

systemctl disable --now acer-profile.service 2>/dev/null || true
