#!/usr/bin/env bash
set -Eeuo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DIST_DIR="$SCRIPT_DIR/dist"
PACKAGE_DIR=""
PACKAGE_NAME=""
PACKAGE_PATH=""

FEATURES="logging,local-http-rustls,local-tun,multi-threaded,aead-cipher,aead-cipher-extra,aead-cipher-2022,aead-cipher-2022-extra,stream-cipher"
ARCH=""
TARGET=""

usage() {
  cat <<'EOF'
Usage: build.sh [--arch amd64|arm64]

Builds a Linux systemd package. The OS target is always Linux.
If --arch is omitted, the current host architecture is used.

Prerequisite:
  cargo install cargo-zigbuild
EOF
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) echo "amd64" ;;
    aarch64|arm64) echo "arm64" ;;
    *)
      echo "Unsupported host architecture: $(uname -m)" >&2
      exit 1
      ;;
  esac
}

parse_args() {
  while (($# > 0)); do
    case "$1" in
      --arch)
        if [[ -z "${2:-}" ]]; then
          echo "--arch requires amd64 or arm64." >&2
          exit 1
        fi
        ARCH="$2"
        shift 2
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      *)
        echo "Unknown argument: $1" >&2
        usage >&2
        exit 1
        ;;
    esac
  done

  ARCH="${ARCH:-$(detect_arch)}"
  case "$ARCH" in
    amd64)
      TARGET="x86_64-unknown-linux-gnu"
      ;;
    arm64)
      TARGET="aarch64-unknown-linux-gnu"
      ;;
    *)
      echo "Unsupported architecture: $ARCH. Supported values: amd64, arm64." >&2
      exit 1
      ;;
  esac
  PACKAGE_NAME="socks-linux-$ARCH"
  PACKAGE_DIR="$SCRIPT_DIR/$PACKAGE_NAME"
  PACKAGE_PATH="$SCRIPT_DIR/$PACKAGE_NAME.tar.gz"
}

parse_args "$@"

if ! command -v cargo >/dev/null 2>&1; then
  echo "cargo is required to build shadowsocks-rust." >&2
  exit 1
fi

if ! command -v cargo-zigbuild >/dev/null 2>&1; then
  echo "cargo-zigbuild is required. Install it with:" >&2
  echo "  cargo install cargo-zigbuild" >&2
  exit 1
fi

if command -v rustup >/dev/null 2>&1; then
  rustup target add "$TARGET"
fi

rm -rf "$DIST_DIR" "$PACKAGE_DIR" "$PACKAGE_PATH"
mkdir -p "$DIST_DIR/bin"

cargo zigbuild \
  --manifest-path "$ROOT_DIR/shadowsocks-rust/Cargo.toml" \
  --locked \
  --release \
  --target "$TARGET" \
  --no-default-features \
  --features "$FEATURES" \
  --bin sslocal

SSLOCAL_SRC="$ROOT_DIR/shadowsocks-rust/target/$TARGET/release/sslocal"
install -m 0755 "$SSLOCAL_SRC" "$DIST_DIR/bin/sslocal"
install -m 0755 "$SCRIPT_DIR/install.sh" "$DIST_DIR/install.sh"
install -m 0755 "$SCRIPT_DIR/socks-run.sh" "$DIST_DIR/socks-run.sh"
install -m 0755 "$SCRIPT_DIR/socks-cleanup.sh" "$DIST_DIR/socks-cleanup.sh"
install -m 0644 "$SCRIPT_DIR/socks.service" "$DIST_DIR/socks.service"
install -m 0644 "$SCRIPT_DIR/socks.env" "$DIST_DIR/socks.env"
install -m 0644 "$SCRIPT_DIR/shadowsocks.acl" "$DIST_DIR/shadowsocks.acl"

cp -R "$DIST_DIR" "$PACKAGE_DIR"
tar -C "$SCRIPT_DIR" -czf "$PACKAGE_PATH" "$PACKAGE_NAME"
rm -rf "$PACKAGE_DIR"

echo "Package directory created at $DIST_DIR for linux/$ARCH ($TARGET)."
echo "Package archive created at $PACKAGE_PATH."
