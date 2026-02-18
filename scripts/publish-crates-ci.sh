#!/usr/bin/env bash
set -euo pipefail

# Non-interactive crates publish helper for CI.
# Supports dry runs and skip-on-already-published behavior.
#
# Usage:
#   ./scripts/publish-crates-ci.sh --dry-run
#   ./scripts/publish-crates-ci.sh

DRY_RUN=false
ALLOW_DIRTY=false
LOG_FILE="${PUBLISH_CRATES_LOG:-publish-crates.log}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --allow-dirty)
      ALLOW_DIRTY=true
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

CRATES=(
  "crates/tandem-types"
  "crates/tandem-wire"
  "crates/tandem-observability"
  "crates/tandem-memory"
  "crates/tandem-skills"
  "crates/tandem-tools"
  "crates/tandem-providers"
  "crates/tandem-orchestrator"
  "crates/tandem-core"
  "crates/tandem-runtime"
  "crates/tandem-server"
  "crates/tandem-tui"
  "engine"
)

mkdir -p "$(dirname "$LOG_FILE")"
: > "$LOG_FILE"

echo "Publishing crates in deterministic order..." | tee -a "$LOG_FILE"
if [[ "$DRY_RUN" == "true" ]]; then
  echo "Mode: dry-run" | tee -a "$LOG_FILE"
fi

publish_args=()
if [[ "$ALLOW_DIRTY" == "true" ]]; then
  publish_args+=(--allow-dirty)
fi

for crate in "${CRATES[@]}"; do
  if [[ ! -d "$crate" ]]; then
    echo "SKIP $crate (missing directory)" | tee -a "$LOG_FILE"
    continue
  fi

  echo "---------------------------------------------------" | tee -a "$LOG_FILE"
  echo "Processing $crate" | tee -a "$LOG_FILE"

  if [[ "$DRY_RUN" == "true" ]]; then
    set +e
    output="$(
      cd "$crate" &&
        cargo package --allow-dirty --no-verify 2>&1
    )"
    code=$?
    set -e
  else
    set +e
    output="$(
      cd "$crate" &&
        cargo publish "${publish_args[@]}" 2>&1
    )"
    code=$?
    set -e
  fi

  echo "$output" | tee -a "$LOG_FILE"

  if [[ $code -ne 0 ]]; then
    if echo "$output" | grep -q "already exists on crates.io index"; then
      echo "SKIP $crate (already published)" | tee -a "$LOG_FILE"
      continue
    fi
    echo "FAIL $crate" | tee -a "$LOG_FILE"
    exit $code
  fi

  echo "OK $crate" | tee -a "$LOG_FILE"
  # Give crates.io index time to propagate.
  sleep 10
done

echo "Crates publish flow completed." | tee -a "$LOG_FILE"
