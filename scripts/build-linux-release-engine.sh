#!/usr/bin/env bash
set -euo pipefail

# Two architecture-specific base digests freeze Rust 1.95.0 and the complete
# Ubuntu 22.04 compiler, linker, glibc, pkg-config, and development environment.
readonly BUILDER_DOCKERFILE="scripts/linux-release-builder.Dockerfile"
readonly BUILDER_TAG="tandem-linux-release-builder:rust-1.95.0-jammy"
readonly TARGET="x86_64-unknown-linux-gnu"
readonly TARGET_PLATFORM="linux/amd64"
readonly ORT_ARCHIVE_URL="https://parcel.pyke.io/v2/delivery/ortrs/packages/msort-binary/1.20.0/ortrs_static-v1.20.0-x86_64-unknown-linux-gnu.tgz"
readonly ORT_ARCHIVE_SHA256="f88a8c1e4b4813a1cfa79af3f35b23addf2f0f36e66c5cd7c88103cb9b30509d"
readonly ORT_CACHE_KEY="${ORT_ARCHIVE_SHA256^^}"

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
  if [[ "$mode" == "with-enterprise" ]]; then
    ort_archive="$cargo_home/onnxruntime-$ORT_ARCHIVE_SHA256.tgz"
    curl --proto '=https' --tlsv1.2 --fail --location --retry 3 \
      --output "$ort_archive" "$ORT_ARCHIVE_URL"
    printf '%s  %s\n' "$ORT_ARCHIVE_SHA256" "$ort_archive" | sha256sum -c -
  fi
  mkdir -p "$root_dir/target"
  docker build --pull --network none --platform "$TARGET_PLATFORM" \
    --file "$root_dir/$BUILDER_DOCKERFILE" \
    --tag "$BUILDER_TAG" \
    "$root_dir/scripts"
  builder_image_id="$(docker image inspect --format '{{.Id}}' "$BUILDER_TAG")"
  [[ "$builder_image_id" == sha256:* ]]

  docker run --rm --platform "$TARGET_PLATFORM" \
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
    --env XDG_CACHE_HOME=/cargo-home/ort-cache \
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
    "$builder_image_id" \
    bash scripts/build-linux-release-engine.sh "$mode"
  "$root_dir/target/$TARGET/release/tandem-engine" --version
  if [[ "$mode" == "with-enterprise" ]]; then
    "$root_dir/target/$TARGET/release/tandem-engine-enterprise" --version
  fi
  exit 0
fi

readonly TARGET_DIR="target/pinned-linux-release"
readonly BUILD_DIR="$TARGET_DIR/$TARGET/release"
readonly OUTPUT_DIR="target/$TARGET/release"

rustc --version
cc --version
ld --version
pkg-config --version
ldd --version

if [[ "$mode" == "with-enterprise" ]]; then
  readonly ORT_ARCHIVE="/cargo-home/onnxruntime-$ORT_ARCHIVE_SHA256.tgz"
  readonly ORT_CACHE_DIR="$XDG_CACHE_HOME/dfbin/$TARGET/$ORT_CACHE_KEY"
  test -f "$ORT_ARCHIVE"
  printf '%s  %s\n' "$ORT_ARCHIVE_SHA256" "$ORT_ARCHIVE" | sha256sum -c -
  mkdir -p "$ORT_CACHE_DIR"
  tar --extract --gzip --file "$ORT_ARCHIVE" --directory "$ORT_CACHE_DIR"
  test -f "$ORT_CACHE_DIR/onnxruntime/lib/libonnxruntime.a"
fi

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
