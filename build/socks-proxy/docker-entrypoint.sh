#!/bin/sh
set -eu

generate_stream_conf() {
  : "${SHADOWSOCKS_SERVER:?SHADOWSOCKS_SERVER is required.}"
  shadowsocks_port="${SHADOWSOCKS_PORT:-39036}"

  cat <<EOF
stream {
	upstream group {
EOF

  for server in $(printf '%s' "$SHADOWSOCKS_SERVER" | tr -d '"'); do
    cat <<EOF
		server $server:$shadowsocks_port;
EOF
  done

  cat <<EOF
	}
	server {
		listen $shadowsocks_port;
		listen $shadowsocks_port udp;
		proxy_pass group;
	}
}
EOF
}

mkdir -p /etc/nginx/stream-conf.d
generate_stream_conf > /etc/nginx/stream-conf.d/shadowsocks.conf

exec "$@"
