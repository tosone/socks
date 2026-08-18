#!/usr/bin/env bash
set -Eeuo pipefail

CONFIG_FILE="${SOCKS_CONFIG:-/etc/socks/socks.env}"

if [[ ! -r "$CONFIG_FILE" ]]; then
  echo "Configuration file is not readable: $CONFIG_FILE" >&2
  exit 1
fi

# shellcheck source=/dev/null
source "$CONFIG_FILE"

: "${SS_SERVER:?SS_SERVER is required.}"
: "${SS_PORT:?SS_PORT is required.}"
: "${SS_METHOD:?SS_METHOD is required.}"
: "${SS_PASSWORD:?SS_PASSWORD is required.}"

TUN_NAME="${TUN_NAME:-socks0}"
TUN_ADDRESS="${TUN_ADDRESS:-10.255.0.1/24}"
SSLOCAL_BIN="${SSLOCAL_BIN:-/opt/socks/bin/sslocal}"
ACL_PATH="${ACL_PATH:-/etc/socks/shadowsocks.acl}"
SS_PLUGIN="${SS_PLUGIN:-/opt/socks/bin/v2ray-plugin}"
SS_PLUGIN_DOMAIN="${SS_PLUGIN_DOMAIN:-}"

SSLOCAL_PID=""
SSLOCAL_PASSWORD=""
RESOLVED_SERVER_IP=""
DEFAULT_GATEWAY=""
DEFAULT_IFACE=""

require_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Required command not found: $1" >&2
    exit 1
  fi
}

ss_key_len() {
  case "$1" in
    2022-blake3-aes-128-gcm) echo 16 ;;
    2022-*) echo 32 ;;
    *) echo 0 ;;
  esac
}

is_base64_key() {
  local key_len="$1"
  local key_value="$2"
  local key_tmp
  local decoded_len

  key_tmp="$(mktemp)"
  if printf '%s' "$key_value" | base64 -d > "$key_tmp" 2>/dev/null; then
    decoded_len="$(wc -c < "$key_tmp" | tr -d ' ')"
    rm -f "$key_tmp"
    [[ "$decoded_len" == "$key_len" ]]
    return
  fi
  rm -f "$key_tmp"
  return 1
}

normalize_ss_password() {
  local key_len

  key_len="$(ss_key_len "$SS_METHOD")"
  if [[ "$key_len" == "0" ]]; then
    SSLOCAL_PASSWORD="$SS_PASSWORD"
    return
  fi

  require_command openssl
  require_command base64

  if is_base64_key "$key_len" "$SS_PASSWORD"; then
    SSLOCAL_PASSWORD="$SS_PASSWORD"
    return
  fi

  SSLOCAL_PASSWORD="$(printf '%s' "$SS_PASSWORD" | openssl dgst -sha256 -binary | head -c "$key_len" | base64 | tr -d '\n')"
}

load_default_route() {
  local default_route
  default_route="$(ip -4 route show default | head -n 1 || true)"
  DEFAULT_GATEWAY="$(awk '{for (i = 1; i <= NF; i++) if ($i == "via") print $(i + 1)}' <<<"$default_route")"
  DEFAULT_IFACE="$(awk '{for (i = 1; i <= NF; i++) if ($i == "dev") print $(i + 1)}' <<<"$default_route")"

  if [[ -z "$DEFAULT_IFACE" ]]; then
    echo "No IPv4 default route found." >&2
    exit 1
  fi
}

resolve_server_ip() {
  if [[ "$SS_SERVER" =~ ^([0-9]{1,3}\.){3}[0-9]{1,3}$ ]]; then
    RESOLVED_SERVER_IP="$SS_SERVER"
    return
  fi

  RESOLVED_SERVER_IP="$(getent ahostsv4 "$SS_SERVER" | awk 'NR == 1 {print $1}')"
  if [[ -z "$RESOLVED_SERVER_IP" ]]; then
    echo "Failed to resolve IPv4 address for SS_SERVER=$SS_SERVER." >&2
    exit 1
  fi
}

cleanup_routes() {
  ip route del 0.0.0.0/1 dev "$TUN_NAME" 2>/dev/null || true
  ip route del 128.0.0.0/1 dev "$TUN_NAME" 2>/dev/null || true

  if [[ -n "$RESOLVED_SERVER_IP" ]]; then
    ip route del "$RESOLVED_SERVER_IP/32" 2>/dev/null || true
  fi
}

cleanup_tun() {
  ip link set "$TUN_NAME" down 2>/dev/null || true
  ip tuntap del mode tun dev "$TUN_NAME" 2>/dev/null || true
}

cleanup() {
  cleanup_routes
  cleanup_tun
}

stop() {
  if [[ -n "$SSLOCAL_PID" ]] && kill -0 "$SSLOCAL_PID" 2>/dev/null; then
    kill "$SSLOCAL_PID" 2>/dev/null || true
    wait "$SSLOCAL_PID" 2>/dev/null || true
  fi
  cleanup
}

setup_tun() {
  modprobe tun 2>/dev/null || true

  cleanup

  ip tuntap add mode tun dev "$TUN_NAME"
  ip addr add "$TUN_ADDRESS" dev "$TUN_NAME"
  ip link set "$TUN_NAME" up
}

setup_routes() {
  if [[ -n "$DEFAULT_GATEWAY" ]]; then
    ip route replace "$RESOLVED_SERVER_IP/32" via "$DEFAULT_GATEWAY" dev "$DEFAULT_IFACE"
  else
    ip route replace "$RESOLVED_SERVER_IP/32" dev "$DEFAULT_IFACE"
  fi

  ip route replace 0.0.0.0/1 dev "$TUN_NAME"
  ip route replace 128.0.0.0/1 dev "$TUN_NAME"
}

start_sslocal() {
  local args=(
    --log-without-time
    --protocol tun
    --server-addr "${SS_SERVER}:${SS_PORT}"
    --encrypt-method "$SS_METHOD"
    --password "$SSLOCAL_PASSWORD"
    --tun-interface-name "$TUN_NAME"
    --acl "$ACL_PATH"
  )

  if [[ -n "$SS_PLUGIN_DOMAIN" ]]; then
    if [[ ! -x "$SS_PLUGIN" ]]; then
      echo "Plugin is not executable: $SS_PLUGIN" >&2
      exit 1
    fi
    args+=(--plugin "$SS_PLUGIN")
    args+=(--plugin-opts "tls;host=$SS_PLUGIN_DOMAIN")
  fi

  "$SSLOCAL_BIN" "${args[@]}" &
  SSLOCAL_PID="$!"
  wait "$SSLOCAL_PID"
}

require_command ip
require_command getent
normalize_ss_password

if [[ ! -x "$SSLOCAL_BIN" ]]; then
  echo "sslocal is not executable: $SSLOCAL_BIN" >&2
  exit 1
fi

if [[ ! -r "$ACL_PATH" ]]; then
  echo "ACL file is not readable: $ACL_PATH" >&2
  exit 1
fi

trap stop INT TERM
trap cleanup EXIT

load_default_route
resolve_server_ip
setup_tun
setup_routes
start_sslocal
