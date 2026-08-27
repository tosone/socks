#!/bin/sh
set -eu

generate_stream_conf() {
  : "${SHADOWSOCKS_SERVER:?SHADOWSOCKS_SERVER is required.}"
  shadowsocks_ports=$(printf '%s' "${SHADOWSOCKS_PORT:-39036}" | tr ',"' '  ')
  shadowsocks_servers=$(printf '%s' "$SHADOWSOCKS_SERVER" | tr -d '"')

  cat <<EOF
stream {
EOF

  for port in $shadowsocks_ports; do
    cat <<EOF
	upstream group_$port {
EOF

    for server in $shadowsocks_servers; do
      cat <<EOF
		server $server:$port;
EOF
    done

    cat <<EOF
	}
	server {
		listen $port;
		listen $port udp;
		proxy_pass group_$port;
	}
EOF
  done

  cat <<EOF
}
EOF
}

mkdir -p /etc/nginx/stream-conf.d
generate_stream_conf > /etc/nginx/stream-conf.d/shadowsocks.conf

exec "$@"
