#!/bin/sh
set -eu

ss_key_len() {
  case "$1" in
    2022-blake3-aes-128-gcm) echo 16 ;;
    2022-*) echo 32 ;;
    *) echo 0 ;;
  esac
}

is_base64_key() {
  key_len="$1"
  key_value="$2"
  key_tmp="$(mktemp)"
  if printf '%s' "$key_value" | base64 -d > "$key_tmp" 2>/dev/null; then
    decoded_len="$(wc -c < "$key_tmp" | tr -d ' ')"
    rm -f "$key_tmp"
    [ "$decoded_len" = "$key_len" ]
    return
  fi
  rm -f "$key_tmp"
  return 1
}

normalize_ss_password() {
  key_len="$(ss_key_len "$1")"
  if [ "$key_len" = 0 ]; then
    printf '%s' "$2"
    return
  fi
  if is_base64_key "$key_len" "$2"; then
    printf '%s' "$2"
    return
  fi
  printf '%s' "$2" | openssl dgst -sha256 -binary | head -c "$key_len" | base64 | tr -d '\n'
}

ss_method="${SS_METHOD:-2022-blake3-chacha20-poly1305}"
ss_password="$(normalize_ss_password "$ss_method" "${SS_PASSWORD:-change-me}")"

cat > /etc/shadowsocks-rust/config.json <<EOF_CONFIG
{"server":"0.0.0.0","server_port":443,"password":"${ss_password}","timeout":300,"method":"${ss_method}"}
EOF_CONFIG

exec "$@"
