#!/usr/bin/env bash
set -euo pipefail

# Non-interactive npm publish helper for CI.
# Skips packages that are already published at the target version.
#
# Usage:
#   ./scripts/publish-npm-ci.sh --dry-run
#   ./scripts/publish-npm-ci.sh

DRY_RUN=false
PROVENANCE=false
LOG_FILE="${PUBLISH_NPM_LOG:-publish-npm.log}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --provenance)
      PROVENANCE=true
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

PACKAGES=(
  "packages/tandem-engine"
  "packages/tandem-tui"
  "packages/tandem-client-ts"
  "packages/tandem-control-panel"
)

mkdir -p "$(dirname "$LOG_FILE")"
: > "$LOG_FILE"

echo "Publishing npm wrappers..." | tee -a "$LOG_FILE"
if [[ "$DRY_RUN" == "true" ]]; then
  echo "Mode: dry-run" | tee -a "$LOG_FILE"
fi

wait_for_npm_version() {
  local name="$1"
  local version="$2"
  local attempts="${3:-20}"
  local delay="${4:-15}"

  for ((i = 1; i <= attempts; i += 1)); do
    if npm view "${name}@${version}" version >/dev/null 2>&1; then
      echo "Confirmed ${name}@${version} on npm" | tee -a "$LOG_FILE"
      return 0
    fi
    echo "Waiting for ${name}@${version} to appear on npm (${i}/${attempts})..." | tee -a "$LOG_FILE"
    sleep "$delay"
  done

  echo "Timed out waiting for ${name}@${version} to appear on npm" | tee -a "$LOG_FILE"
  return 1
}

write_token_npmrc() {
  local file="$1"
  cat >"$file" <<EOF
registry=https://registry.npmjs.org/
@frumu:registry=https://registry.npmjs.org/
//registry.npmjs.org/:_authToken=${NPM_TOKEN}
always-auth=true
EOF
}

run_npm_publish() {
  local dir="$1"
  local userconfig="$2"
  shift 2

  local output_file
  output_file="$(mktemp)"
  local status=0

  set +e
  if [[ -n "$userconfig" ]]; then
    (cd "$dir" && NPM_CONFIG_USERCONFIG="$userconfig" "$@") >"$output_file" 2>&1
  else
    (cd "$dir" && "$@") >"$output_file" 2>&1
  fi
  status=$?
  set -e

  cat "$output_file" | tee -a "$LOG_FILE"
  rm -f "$output_file"
  return "$status"
}

auth_error_detected() {
  local output="$1"
  printf '%s' "$output" | grep -Eqi 'ENEEDAUTH|need auth|E401|E403|E404|Unauthorized'
}

for dir in "${PACKAGES[@]}"; do
  if [[ ! -d "$dir" ]]; then
    echo "SKIP $dir (missing directory)" | tee -a "$LOG_FILE"
    continue
  fi

  name=$(node -p "require('./$dir/package.json').name")
  version=$(node -p "require('./$dir/package.json').version")
  echo "Processing $name@$version ($dir)" | tee -a "$LOG_FILE"

  if npm view "${name}@${version}" version >/dev/null 2>&1; then
    echo "SKIP $name@$version already published" | tee -a "$LOG_FILE"
    continue
  fi

  publish_cmd=(npm publish --access public)
  if [[ "$PROVENANCE" == "true" ]]; then
    publish_cmd+=(--provenance)
  fi

  # TS SDK publish path: build explicitly, then publish without lifecycle scripts.
  # This avoids npm workspace dependency resolution failures in CI.
  if [[ "$dir" == "packages/tandem-client-ts" ]]; then
    echo "Building JS bundles for $name@$version with npx tsup" | tee -a "$LOG_FILE"
    (
      cd "$dir" &&
        npx --yes -p tsup -p typescript -p zod tsup src/index.ts --format esm,cjs --clean
    ) 2>&1 | tee -a "$LOG_FILE"
    echo "Building type declarations for $name@$version with npx tsc" | tee -a "$LOG_FILE"
    (
      cd "$dir" &&
        npx --yes -p typescript tsc --project tsconfig.json --emitDeclarationOnly
    ) 2>&1 | tee -a "$LOG_FILE"
    publish_cmd+=(--ignore-scripts)
  fi

  # Control panel publish path: build static bundle explicitly, then publish without lifecycle scripts.
  if [[ "$dir" == "packages/tandem-control-panel" ]]; then
    wait_for_npm_version "@frumu/tandem" "$version"
    wait_for_npm_version "@frumu/tandem-client" "$version"
    if command -v pnpm >/dev/null 2>&1; then
      echo "Installing panel dependencies for $name@$version with pnpm (refresh lock metadata)" | tee -a "$LOG_FILE"
      (
        cd "$dir" &&
          pnpm install --no-frozen-lockfile
      ) 2>&1 | tee -a "$LOG_FILE"
      echo "Building static bundle for $name@$version with pnpm run build" | tee -a "$LOG_FILE"
      (
        cd "$dir" &&
          pnpm run build
      ) 2>&1 | tee -a "$LOG_FILE"
    else
      echo "Building static bundle for $name@$version with npx vite build (fallback)" | tee -a "$LOG_FILE"
      (
        cd "$dir" &&
          npx --yes -p vite -p @frumu/tandem-client -p tailwindcss -p autoprefixer -p @tailwindcss/forms vite build
      ) 2>&1 | tee -a "$LOG_FILE"
    fi
    publish_cmd+=(--ignore-scripts)
  fi

  publish_userconfig="${NPM_CONFIG_USERCONFIG:-}"
  publish_output=""
  publish_status=0
  fallback_userconfig=""

  if [[ "$DRY_RUN" == "true" ]]; then
    if publish_output="$(run_npm_publish "$dir" "$publish_userconfig" "${publish_cmd[@]}" --dry-run)"; then
      publish_status=0
    else
      publish_status=$?
    fi
  else
    if publish_output="$(run_npm_publish "$dir" "$publish_userconfig" "${publish_cmd[@]}")"; then
      publish_status=0
    else
      publish_status=$?
    fi
  fi

  if [[ "$publish_status" -ne 0 && "$DRY_RUN" != "true" && -n "${NPM_TOKEN:-}" ]]; then
    if auth_error_detected "$publish_output"; then
      echo "Retrying $name@$version with token auth after npm auth failure" | tee -a "$LOG_FILE"
      fallback_userconfig="$(mktemp)"
      write_token_npmrc "$fallback_userconfig"
      if publish_output="$(run_npm_publish "$dir" "$fallback_userconfig" "${publish_cmd[@]}")"; then
        publish_status=0
      else
        publish_status=$?
      fi
      rm -f "$fallback_userconfig"
    fi
  fi

  if [[ "$publish_status" -ne 0 ]]; then
    echo "Publish failed for $name@$version" | tee -a "$LOG_FILE"
    exit "$publish_status"
  fi

  echo "OK $name@$version" | tee -a "$LOG_FILE"
done

echo "npm publish flow completed." | tee -a "$LOG_FILE"
