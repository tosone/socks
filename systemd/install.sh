#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -x "$SCRIPT_DIR/bin/sslocal" ]]; then
  DIST_DIR="$SCRIPT_DIR"
else
  DIST_DIR="$SCRIPT_DIR/dist"
fi
INSTALL_DIR="/opt/socks"
CONFIG_DIR="/etc/socks"
SERVICE_PATH="/etc/systemd/system/socks.service"
CONFIG_PATH="$CONFIG_DIR/socks.env"
ACL_PATH="$CONFIG_DIR/shadowsocks.acl"

METHOD_OPTIONS=(
  "2022-blake3-chacha20-poly1305"
  "2022-blake3-aes-256-gcm"
  "2022-blake3-aes-128-gcm"
  "2022-blake3-chacha8-poly1305"
  "xchacha20-ietf-poly1305"
  "chacha20-ietf-poly1305"
  "chacha20-ietf"
  "aes-256-gcm"
  "aes-128-gcm"
  "aes-256-gcm-siv"
  "aes-128-gcm-siv"
  "aes-256-ccm"
  "aes-128-ccm"
  "aes-256-ctr"
  "aes-192-ctr"
  "aes-128-ctr"
  "aes-256-cfb"
  "aes-192-cfb"
  "aes-128-cfb"
  "aes-256-cfb1"
  "aes-192-cfb1"
  "aes-128-cfb1"
  "aes-256-cfb8"
  "aes-192-cfb8"
  "aes-128-cfb8"
  "aes-256-ofb"
  "aes-192-ofb"
  "aes-128-ofb"
  "sm4-gcm"
  "sm4-ccm"
  "camellia-128-ctr"
  "camellia-192-ctr"
  "camellia-256-ctr"
  "camellia-128-cfb"
  "camellia-192-cfb"
  "camellia-256-cfb"
  "camellia-128-cfb1"
  "camellia-192-cfb1"
  "camellia-256-cfb1"
  "camellia-128-cfb8"
  "camellia-192-cfb8"
  "camellia-256-cfb8"
  "camellia-128-ofb"
  "camellia-192-ofb"
  "camellia-256-ofb"
  "rc4-md5"
  "rc4"
  "table"
)

prompt_required() {
  local var_name="$1"
  local prompt="$2"
  local value="${!var_name:-}"

  while [[ -z "$value" ]]; do
    read -r -p "$prompt: " value
  done

  export "$var_name=$value"
}

prompt_default() {
  local var_name="$1"
  local prompt="$2"
  local default_value="$3"
  local value

  read -r -p "$prompt [$default_value]: " value
  export "$var_name=${value:-$default_value}"
}

prompt_secret_required() {
  local var_name="$1"
  local prompt="$2"
  local value="${!var_name:-}"

  while [[ -z "$value" ]]; do
    read -r -s -p "$prompt: " value
    echo
  done

  export "$var_name=$value"
}

prompt_optional() {
  local var_name="$1"
  local prompt="$2"
  local default_value="$3"
  local value

  read -r -p "$prompt [$default_value]: " value
  export "$var_name=${value:-$default_value}"
}

choose_method() {
  local current="${SS_METHOD:-${METHOD_OPTIONS[0]}}"
  local choice

  echo "Encryption method:"
  for index in "${!METHOD_OPTIONS[@]}"; do
    printf "  %d) %s\n" "$((index + 1))" "${METHOD_OPTIONS[$index]}"
  done
  echo "  c) Custom"

  read -r -p "Choose method [1]: " choice
  case "${choice:-1}" in
    c|C)
      prompt_required SS_METHOD "Custom encryption method"
      ;;
    ''|*[!0-9]*)
      export SS_METHOD="$current"
      ;;
    *)
      if (( choice >= 1 && choice <= ${#METHOD_OPTIONS[@]} )); then
        export SS_METHOD="${METHOD_OPTIONS[$((choice - 1))]}"
      else
        export SS_METHOD="$current"
      fi
      ;;
  esac
}

collect_config() {
  if [[ ! -t 0 || "${SOCKS_INSTALL_NONINTERACTIVE:-}" == "1" ]]; then
    : "${SS_SERVER:?SS_SERVER is required in non-interactive mode.}"
    : "${SS_PASSWORD:?SS_PASSWORD is required in non-interactive mode.}"
    export SS_PORT="${SS_PORT:-443}"
    export SS_METHOD="${SS_METHOD:-${METHOD_OPTIONS[0]}}"
    export TUN_NAME="${TUN_NAME:-socks0}"
    export TUN_ADDRESS="${TUN_ADDRESS:-10.255.0.1/24}"
    export SS_PLUGIN_DOMAIN="${SS_PLUGIN_DOMAIN:-}"
    return
  fi

  echo "Configure socks systemd service."
  prompt_required SS_SERVER "Server IP or domain"
  prompt_default SS_PORT "Server port" "${SS_PORT:-443}"
  choose_method
  prompt_secret_required SS_PASSWORD "Server password"
  prompt_default TUN_NAME "TUN interface name" "${TUN_NAME:-socks0}"
  prompt_default TUN_ADDRESS "TUN interface address" "${TUN_ADDRESS:-10.255.0.1/24}"
  prompt_optional SS_PLUGIN_DOMAIN "Plugin domain, empty to disable" "${SS_PLUGIN_DOMAIN:-}"
}

write_config_from_env() {
  umask 077
  {
    printf "SS_SERVER=%q\n" "$SS_SERVER"
    printf "SS_PORT=%q\n" "${SS_PORT:-443}"
    printf "SS_METHOD=%q\n" "${SS_METHOD:-2022-blake3-chacha20-poly1305}"
    printf "SS_PASSWORD=%q\n" "$SS_PASSWORD"
    printf "\n"
    printf "TUN_NAME=%q\n" "${TUN_NAME:-socks0}"
    printf "TUN_ADDRESS=%q\n" "${TUN_ADDRESS:-10.255.0.1/24}"
    printf "\n"
    printf "SSLOCAL_BIN=%q\n" "$INSTALL_DIR/bin/sslocal"
    printf "ACL_PATH=%q\n" "$ACL_PATH"
    if [[ -n "${SS_PLUGIN_DOMAIN:-}" ]]; then
      printf "SS_PLUGIN_DOMAIN=%q\n" "$SS_PLUGIN_DOMAIN"
    fi
  } > "$CONFIG_PATH"
  chmod 0600 "$CONFIG_PATH"
}

install_files() {
  if [[ ! -x "$DIST_DIR/bin/sslocal" ]]; then
    echo "Missing build artifact: $DIST_DIR/bin/sslocal" >&2
    exit 1
  fi

  install -d -m 0755 "$INSTALL_DIR/bin" "$CONFIG_DIR"
  install -m 0755 "$DIST_DIR/bin/sslocal" "$INSTALL_DIR/bin/sslocal"
  if [[ -x "$DIST_DIR/bin/v2ray-plugin" ]]; then
    install -m 0755 "$DIST_DIR/bin/v2ray-plugin" "$INSTALL_DIR/bin/v2ray-plugin"
  fi
  install -m 0755 "$DIST_DIR/socks-run.sh" "$INSTALL_DIR/socks-run.sh"
  install -m 0644 "$DIST_DIR/socks.service" "$SERVICE_PATH"
  if [[ ! -f "$ACL_PATH" ]]; then
    install -m 0644 "$DIST_DIR/shadowsocks.acl" "$ACL_PATH"
  fi

  if [[ -n "${SS_SERVER:-}" && -n "${SS_PASSWORD:-}" ]]; then
    write_config_from_env
  elif [[ ! -f "$CONFIG_PATH" ]]; then
    install -m 0600 "$DIST_DIR/socks.env" "$CONFIG_PATH"
  fi

  systemctl daemon-reload
}

maybe_start_service() {
  if grep -Eq '^SS_SERVER="?198\.51\.100\.10"?$' "$CONFIG_PATH" || grep -Eq '^SS_PASSWORD="?change-me"?$' "$CONFIG_PATH"; then
    echo "Installed but not started. Edit $CONFIG_PATH, then run:" >&2
    echo "  sudo systemctl enable --now socks.service" >&2
    return
  fi

  systemctl enable --now socks.service
  systemctl --no-pager --full status socks.service || true
}

if [[ "${1:-}" != "--install-only" ]]; then
  collect_config
  if [[ ! -x "$DIST_DIR/bin/sslocal" ]]; then
    "$SCRIPT_DIR/build.sh"
  fi
  if [[ "$EUID" -ne 0 ]]; then
    exec sudo -E bash "$0" --install-only
  fi
fi

if [[ "$EUID" -ne 0 ]]; then
  echo "Root privileges are required for installation." >&2
  exit 1
fi

install_files
maybe_start_service
