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
      --features "server,logging,local-http-rustls,multi-threaded,aead-cipher,aead-cipher-extra,aead-cipher-2022,aead-cipher-2022-extra,stream-cipher" \
      --bin ssserver \
    && cp "target/${rust_target}/release/ssserver" /usr/local/bin/ssserver

FROM alpine:3.24

RUN apk add --no-cache ca-certificates openssl tzdata \
    && mkdir -p /etc/shadowsocks-rust

COPY --from=shadowsocks-builder /usr/local/bin/ssserver /usr/local/bin/ssserver
COPY build/socks-server/docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh

RUN chmod +x /usr/local/bin/docker-entrypoint.sh /usr/local/bin/ssserver

EXPOSE 39036/tcp

ENTRYPOINT ["docker-entrypoint.sh"]
CMD ["ssserver", "--log-without-time", "-a", "nobody", "-c", "/etc/shadowsocks-rust/config.json"]

# sslocal -b 127.0.0.1:1080 -s example.com:39036 -m "$SS_METHOD" -k "$SS_PASSWORD"
# docker buildx build --platform linux/amd64,linux/arm64 -f build/socks-server/Dockerfile -t socks-server:latest .
# docker run --rm -e SS_PASSWORD=change-me -e SS_METHOD=2022-blake3-chacha20-poly1305 -p 39036:39036/tcp socks-server:latest
