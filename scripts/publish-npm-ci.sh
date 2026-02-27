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
)

mkdir -p "$(dirname "$LOG_FILE")"
: > "$LOG_FILE"

echo "Publishing npm wrappers..." | tee -a "$LOG_FILE"
if [[ "$DRY_RUN" == "true" ]]; then
  echo "Mode: dry-run" | tee -a "$LOG_FILE"
fi

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

  if [[ "$DRY_RUN" == "true" ]]; then
    (cd "$dir" && "${publish_cmd[@]}" --dry-run) 2>&1 | tee -a "$LOG_FILE"
  else
    (cd "$dir" && "${publish_cmd[@]}") 2>&1 | tee -a "$LOG_FILE"
  fi

  echo "OK $name@$version" | tee -a "$LOG_FILE"
done

echo "npm publish flow completed." | tee -a "$LOG_FILE"
