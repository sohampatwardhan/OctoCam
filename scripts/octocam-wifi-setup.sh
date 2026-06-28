#!/usr/bin/env bash
set -euo pipefail

PROJECT_DIR="${OCTOCAM_PROJECT_DIR:-/opt/octocam}"
WIFI_CACHE_PATH="${OCTOCAM_WIFI_CACHE_PATH:-/var/lib/octocam/wifi-networks.json}"
AP_SSID="${OCTOCAM_SETUP_AP_SSID:-OctoCam-Setup}"
WIFI_IFACE="${OCTOCAM_WIFI_IFACE:-wlan0}"

if ! command -v nmcli >/dev/null 2>&1; then
  echo "NetworkManager CLI is required for OctoCam Wi-Fi setup."
  exit 1
fi

is_real_wifi_connected() {
  local active_name
  active_name="$(nmcli -t -f NAME,DEVICE connection show --active | awk -F: -v iface="$WIFI_IFACE" '$2 == iface {print $1; exit}')"
  [[ -n "$active_name" && "$active_name" != "$AP_SSID" ]]
}

saved_wifi_profiles_by_last_connected() {
  local name
  local profile_type
  local timestamp

  while IFS= read -r name; do
    [[ -n "$name" && "$name" != "$AP_SSID" ]] || continue

    profile_type="$(nmcli -g connection.type connection show "$name" 2>/dev/null || true)"
    [[ "$profile_type" == "802-11-wireless" ]] || continue

    timestamp="$(nmcli -g connection.timestamp connection show "$name" 2>/dev/null || true)"
    [[ "$timestamp" =~ ^[0-9]+$ ]] || timestamp=0

    printf '%s\t%s\n' "$timestamp" "$name"
  done < <(nmcli -g NAME connection show) | sort -rn -k1,1 | cut -f2-
}

try_saved_wifi_profiles() {
  local profile
  local attempted=0

  while IFS= read -r profile; do
    [[ -n "$profile" ]] || continue
    attempted=1

    echo "Trying saved Wi-Fi profile: $profile"
    nmcli connection modify "$AP_SSID" connection.autoconnect no >/dev/null 2>&1 || true

    if nmcli connection up "$profile" ifname "$WIFI_IFACE"; then
      echo "Connected using saved Wi-Fi profile: $profile"
      return 0
    fi
  done < <(saved_wifi_profiles_by_last_connected)

  if [[ "$attempted" -eq 0 ]]; then
    return 2
  fi

  return 1
}

start_setup_ap() {
  echo "Scanning and starting setup AP."

  PYTHONPATH="$PROJECT_DIR" OCTOCAM_WIFI_CACHE_PATH="$WIFI_CACHE_PATH" \
    /usr/bin/python3 -c "from octocam.wifi import scan_and_cache_networks; scan_and_cache_networks()" || true

  if nmcli -t -f NAME connection show | grep -Fxq "$AP_SSID"; then
    nmcli connection modify "$AP_SSID" connection.autoconnect yes
    nmcli connection up "$AP_SSID"
    exit 0
  fi

  nmcli connection add \
    type wifi \
    ifname "$WIFI_IFACE" \
    con-name "$AP_SSID" \
    autoconnect yes \
    ssid "$AP_SSID"

  nmcli connection modify "$AP_SSID" \
    802-11-wireless.mode ap \
    802-11-wireless.band bg \
    ipv4.method shared \
    ipv6.method disabled

  nmcli connection up "$AP_SSID"
}

if is_real_wifi_connected; then
  echo "Wi-Fi is already connected to a saved network; setup AP not needed."
  exit 0
fi

if try_saved_wifi_profiles; then
  exit 0
fi

echo "No saved Wi-Fi profile connected successfully; falling back to setup AP."
start_setup_ap
