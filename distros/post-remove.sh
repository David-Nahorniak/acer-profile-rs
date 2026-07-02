#!/bin/bash
# Post-remove hook pro .deb/.rpm balíčky acer-profile.
# Ponechává PPD masknutý (uživatel může chtít jiný controller) -
# jen informuje o návratu k PPD. State + config zůstávají (smaž ručně).

set -e

systemctl daemon-reload 2>/dev/null || true

echo ":: acer-profile odstraněn. Pro návrat k PPD:"
echo "   sudo systemctl unmask power-profiles-daemon && sudo systemctl enable --now power-profiles-daemon"
echo "   State: /var/lib/acer-profile a config /etc/acer-profile zůstaly (smaž ručně)."
