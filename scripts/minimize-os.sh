#!/usr/bin/env bash
set -euo pipefail

DISABLE_BLUETOOTH=0
DISABLE_MDNS=0
DISABLE_APT_TIMERS=0

for arg in "$@"; do
  case "$arg" in
    --disable-bluetooth)
      DISABLE_BLUETOOTH=1
      ;;
    --disable-mdns)
      DISABLE_MDNS=1
      ;;
    --disable-apt-timers)
      DISABLE_APT_TIMERS=1
      ;;
    -h|--help)
      cat <<'EOF'
Usage: sudo ./scripts/minimize-os.sh [options]

Conservative Raspberry Pi OS Lite minimization for an OctoCam appliance.

Options:
  --disable-bluetooth  Disable Bluetooth services if setup will never use BLE.
  --disable-mdns       Disable Avahi/mDNS; octocam.local will stop working.
  --disable-apt-timers Disable apt daily timers; you must patch manually.
EOF
      exit 0
      ;;
    *)
      echo "Unknown option: $arg"
      exit 1
      ;;
  esac
done

if [[ "${EUID}" -ne 0 ]]; then
  echo "Run with sudo: sudo ./scripts/minimize-os.sh"
  exit 1
fi

disable_unit_if_present() {
  local unit="$1"
  if systemctl cat "$unit" >/dev/null 2>&1; then
    systemctl disable --now "$unit" >/dev/null 2>&1 || true
  fi
}

echo "Applying conservative OctoCam OS minimization..."

install -d /etc/apt/apt.conf.d
cat > /etc/apt/apt.conf.d/99octocam-no-recommends <<'EOF'
APT::Install-Recommends "false";
APT::Install-Suggests "false";
EOF

install -d /etc/systemd/journald.conf.d
cat > /etc/systemd/journald.conf.d/octocam.conf <<'EOF'
[Journal]
SystemMaxUse=32M
RuntimeMaxUse=16M
MaxRetentionSec=7day
Compress=yes
EOF

systemctl set-default multi-user.target

for unit in \
  man-db.timer \
  e2scrub_all.timer; do
  disable_unit_if_present "$unit"
done

for unit in \
  ModemManager.service \
  cups.service \
  cups-browsed.service \
  triggerhappy.service \
  wpa_supplicant@.service; do
  disable_unit_if_present "$unit"
done

if [[ "$DISABLE_BLUETOOTH" -eq 1 ]]; then
  for unit in bluetooth.service hciuart.service; do
    disable_unit_if_present "$unit"
  done
fi

if [[ "$DISABLE_MDNS" -eq 1 ]]; then
  disable_unit_if_present avahi-daemon.service
fi

if [[ "$DISABLE_APT_TIMERS" -eq 1 ]]; then
  for unit in apt-daily.timer apt-daily-upgrade.timer; do
    disable_unit_if_present "$unit"
  done
fi

apt-get autoremove -y
apt-get clean
systemctl restart systemd-journald.service

echo "Minimal OS profile applied."
