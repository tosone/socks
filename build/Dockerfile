FROM rust:1.97.1-alpine3.24 AS shadowsocks-builder

ARG TARGETARCH

RUN apk add --no-cache gcc musl-dev

WORKDIR /build/shadowsocks-rust
COPY shadowsocks-rust/ ./

RUN case "${TARGETARCH}" in \
      amd64) rust_target="x86_64-unknown-linux-musl" ;; \
      arm64) rust_target="aarch64-unknown-linux-musl" ;; \
      *) echo "Unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && rustup target add "${rust_target}" \
    && cargo build \
      --locked \
      --release \
      --target "${rust_target}" \
      --no-default-features \
      --features "server,logging,multi-threaded,aead-cipher,aead-cipher-extra,aead-cipher-2022,aead-cipher-2022-extra,stream-cipher" \
      --bin ssserver \
    && cp "target/${rust_target}/release/ssserver" /usr/local/bin/ssserver

FROM golang:1.26-alpine AS plugin-builder

ARG TARGETARCH
WORKDIR /build/v2ray-plugin
COPY src-tauri/resources/plugins/v2ray-plugin/ ./

RUN case "${TARGETARCH}" in \
      amd64) goarch="amd64" ;; \
      arm64) goarch="arm64" ;; \
      *) echo "Unsupported TARGETARCH: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
    && CGO_ENABLED=0 GOOS=linux GOARCH="${goarch}" \
      go build -trimpath -ldflags "-s -w -buildid=" -o /usr/local/bin/v2ray-plugin

FROM alpine:3.24

RUN apk add --no-cache ca-certificates tzdata \
    && mkdir -p /etc/shadowsocks-rust

COPY --from=shadowsocks-builder /usr/local/bin/ssserver /usr/local/bin/ssserver
COPY --from=plugin-builder /usr/local/bin/v2ray-plugin /usr/local/bin/v2ray-plugin

RUN cat > /usr/local/bin/docker-entrypoint.sh <<'EOF'
#!/bin/sh
set -eu

cat > /etc/shadowsocks-rust/config.json <<EOF_CONFIG
{"server":"0.0.0.0","server_port":443,"password":"${SS_PASSWORD:-change-me}","timeout":300,"method":"${SS_METHOD:-2022-blake3-chacha20-poly1305}","plugin":"v2ray-plugin","plugin_opts":"server;tls;host=${SS_DOMAIN:-example.com}","plugin_mode":"tcp_only"}
EOF_CONFIG

exec "$@"
EOF

RUN chmod +x /usr/local/bin/docker-entrypoint.sh /usr/local/bin/ssserver /usr/local/bin/v2ray-plugin

EXPOSE 443/tcp

ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["ssserver", "--log-without-time", "-a", "nobody", "-c", "/etc/shadowsocks-rust/config.json"]

# sslocal -b 127.0.0.1:1080 -s example.com:443 -m "$SS_METHOD" -k "$SS_PASSWORD" --plugin v2ray-plugin --plugin-opts "tls;host=example.com"
# docker buildx build --platform linux/amd64,linux/arm64 -f build/Dockerfile -t socks-server:latest .
# docker run --rm -e SS_DOMAIN=example.com -e SS_PASSWORD=change-me -e SS_METHOD=2022-blake3-chacha20-poly1305 -p 443:443/tcp socks-server:latest
