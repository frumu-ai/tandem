#!/usr/bin/env bash
set -euo pipefail

# The architecture-specific digest freezes Rust, glibc, GCC, binutils,
# pkg-config, and the remaining Debian Bullseye build environment. Bullseye's
# older glibc plus explicit static OpenSSL linkage keeps the binaries compatible
# with Ubuntu 22.04 while avoiding mutable hosted-runner and apt package inputs.
readonly BUILDER_IMAGE="rust:1.95.0-bullseye@sha256:646e8ceea789b00c5cfa339816a3ed44940dbf1651dc167b78f3c0aefcae0025"
readonly TARGET="x86_64-unknown-linux-gnu"

mode="${1:-standard}"
if [[ "$mode" != "standard" && "$mode" != "with-enterprise" ]]; then
  echo "Usage: $0 [standard|with-enterprise]" >&2
  exit 2
fi

if [[ "${TANDEM_PINNED_LINUX_BUILDER:-}" != "1" ]]; then
  root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  cargo_home="$(mktemp -d "${RUNNER_TEMP:-/tmp}/tandem-release-cargo.XXXXXX")"
  trap 'rm -rf -- "$cargo_home"' EXIT

  # Fetch public, lockfile-pinned inputs with trusted host Cargo into a fresh
  # credential-free cache. The build container receives no network access or
  # host Cargo home, and sees source files read-only.
  CARGO_HOME="$cargo_home" cargo fetch --locked --target "$TARGET"
  rm -f "$cargo_home/credentials" "$cargo_home/credentials.toml"
  mkdir -p "$root_dir/target"

  docker run --rm --pull=always --platform linux/amd64 \
    --hostname tandem-release-builder \
    --network none \
    --read-only \
    --cap-drop ALL \
    --security-opt no-new-privileges \
    --tmpfs /tmp:rw,nosuid,nodev,size=1g \
    --user "$(id -u):$(id -g)" \
    --volume "$root_dir:/workspace:ro" \
    --volume "$root_dir/target:/workspace/target:rw" \
    --volume "$cargo_home:/cargo-home" \
    --workdir /workspace \
    --env CARGO_HOME=/cargo-home \
    --env RUSTUP_HOME=/usr/local/rustup \
    --env CARGO_INCREMENTAL=0 \
    --env OPENSSL_STATIC=1 \
    --env OPENSSL_LIB_DIR=/usr/lib/x86_64-linux-gnu \
    --env OPENSSL_INCLUDE_DIR=/usr/include \
    --env OPENSSL_NO_PKG_CONFIG=1 \
    --env SOURCE_DATE_EPOCH=0 \
    --env TZ=UTC \
    --env LANG=C.UTF-8 \
    --env LC_ALL=C.UTF-8 \
    --env TANDEM_PINNED_LINUX_BUILDER=1 \
    "$BUILDER_IMAGE" \
    bash scripts/build-linux-release-engine.sh "$mode"
  "$root_dir/target/$TARGET/release/tandem-engine" --version
  exit 0
fi

readonly TARGET_DIR="target/pinned-linux-release"
readonly BUILD_DIR="$TARGET_DIR/$TARGET/release"
readonly OUTPUT_DIR="target/$TARGET/release"

rustc --version
cc --version | head -n 1
ld --version | head -n 1
pkg-config --version

cargo clean --target-dir "$TARGET_DIR"
cargo build --offline --locked --release --target "$TARGET" --target-dir "$TARGET_DIR" \
  -p tandem-ai -p tandem-tui -p tandem-browser \
  --features tandem-ai/browser,tandem-ai/enterprise

mkdir -p "$OUTPUT_DIR"
if [[ "$mode" == "with-enterprise" ]]; then
  cargo build --offline --locked --release --target "$TARGET" --target-dir "$TARGET_DIR" \
    -p tandem-ai \
    --features tandem-ai/browser,tandem-ai/enterprise-full
  install -m 0755 "$BUILD_DIR/tandem-engine" "$OUTPUT_DIR/tandem-engine-enterprise"

  # Restore the standard feature composition after the enterprise-full build
  # overwrites the shared tandem-engine output path.
  cargo build --offline --locked --release --target "$TARGET" --target-dir "$TARGET_DIR" \
    -p tandem-ai -p tandem-tui -p tandem-browser \
    --features tandem-ai/browser,tandem-ai/enterprise
fi

install -m 0755 "$BUILD_DIR/tandem-engine" "$OUTPUT_DIR/tandem-engine"
install -m 0755 "$BUILD_DIR/tandem-tui" "$OUTPUT_DIR/tandem-tui"
install -m 0755 "$BUILD_DIR/tandem-browser" "$OUTPUT_DIR/tandem-browser"

release_binaries=(
  "$OUTPUT_DIR/tandem-engine"
  "$OUTPUT_DIR/tandem-tui"
  "$OUTPUT_DIR/tandem-browser"
)
if [[ "$mode" == "with-enterprise" ]]; then
  release_binaries+=("$OUTPUT_DIR/tandem-engine-enterprise")
fi
for binary in "${release_binaries[@]}"; do
  if ldd "$binary" | grep -Eq 'lib(ssl|crypto)\.so'; then
    echo "pinned Linux release binary dynamically links OpenSSL: $binary" >&2
    exit 1
  fi
done

sha256sum "$OUTPUT_DIR/tandem-engine"
