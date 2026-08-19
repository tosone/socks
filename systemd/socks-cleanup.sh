#!/usr/bin/env bash
set -Eeuo pipefail

CONFIG_FILE="${SOCKS_CONFIG:-/etc/socks/socks.env}"

if [[ -r "$CONFIG_FILE" ]]; then
  # shellcheck source=/dev/null
  source "$CONFIG_FILE"
fi

TUN_NAME="${TUN_NAME:-socks0}"
SS_SERVER="${SS_SERVER:-}"
RESOLVED_SERVER_IP="${RESOLVED_SERVER_IP:-}"

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Required command not found: $1" >&2
    exit 1
  fi
}

resolve_server_ip() {
  if [[ -n "$RESOLVED_SERVER_IP" ]]; then
    return
  fi
  if [[ -z "$SS_SERVER" ]]; then
    return
  fi
  if [[ "$SS_SERVER" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]]; then
    RESOLVED_SERVER_IP="$SS_SERVER"
    return
  fi
  RESOLVED_SERVER_IP="$(getent ahostsv4 "$SS_SERVER" | awk 'NR == 1 {print $1}')"
}

delete_route() {
  local description="$1"
  shift

  if "$@" 2>/dev/null; then
    echo "Removed $description."
  else
    echo "Skipped $description."
  fi
}

cleanup_routes() {
  delete_route "0.0.0.0/1 route on $TUN_NAME" \
    ip route del 0.0.0.0/1 dev "$TUN_NAME"
  delete_route "128.0.0.0/1 route on $TUN_NAME" \
    ip route del 128.0.0.0/1 dev "$TUN_NAME"

  if [[ -n "$RESOLVED_SERVER_IP" ]]; then
    delete_route "$RESOLVED_SERVER_IP/32 server route" \
      ip route del "$RESOLVED_SERVER_IP/32"
  fi
}

cleanup_tun() {
  if ip link show "$TUN_NAME" >/dev/null 2>&1; then
    ip link set "$TUN_NAME" down 2>/dev/null || true
    if ip tuntap del mode tun dev "$TUN_NAME" 2>/dev/null; then
      echo "Deleted TUN interface $TUN_NAME."
    else
      echo "Failed to delete TUN interface $TUN_NAME." >&2
      exit 1
    fi
  else
    echo "Skipped TUN interface $TUN_NAME."
  fi
}

require_command ip
require_command getent

resolve_server_ip
cleanup_routes
cleanup_tun
