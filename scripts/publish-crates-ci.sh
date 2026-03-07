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
  "crates/tandem-document"
  "crates/tandem-providers"
  "crates/tandem-memory"
  "crates/tandem-skills"
  "crates/tandem-agent-teams"
  "crates/tandem-tools"
  "crates/tandem-orchestrator"
  "crates/tandem-core"
  "crates/tandem-browser"
  "crates/tandem-runtime"
  "crates/tandem-channels"
  "crates/tandem-server"
  "crates/tandem-tui"
  "engine"
)

crate_name() {
  local crate_dir="$1"
  python - "$crate_dir/Cargo.toml" <<'PY'
import re
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text()
match = re.search(r'(?m)^\s*name\s*=\s*"([^"]+)"', text)
if not match:
    raise SystemExit("Could not find package name")
print(match.group(1))
PY
}

crate_version() {
  local crate_dir="$1"
  python - "$crate_dir/Cargo.toml" <<'PY'
import re
import sys
from pathlib import Path

text = Path(sys.argv[1]).read_text()
match = re.search(r'(?m)^\s*version\s*=\s*"([^"]+)"', text)
if not match:
    raise SystemExit("Could not find package version")
print(match.group(1))
PY
}

wait_for_crate_version() {
  local crate_dir="$1"
  local name
  local version
  name="$(crate_name "$crate_dir")"
  version="$(crate_version "$crate_dir")"

  echo "Waiting for crates.io to expose ${name} ${version}..." | tee -a "$LOG_FILE"

  python - "$name" "$version" <<'PY'
import json
import sys
import time
import urllib.error
import urllib.request

name = sys.argv[1]
version = sys.argv[2]
url = f"https://crates.io/api/v1/crates/{name}"
deadline = time.time() + 180
sleep_secs = 5

while time.time() < deadline:
    try:
        request = urllib.request.Request(
            url,
            headers={
                "User-Agent": "tandem-publish-ci/1.0",
                "Accept": "application/json",
            },
        )
        with urllib.request.urlopen(request, timeout=20) as response:
            payload = json.load(response)
        versions = payload.get("versions", [])
        if any(item.get("num") == version for item in versions):
            print(f"Confirmed {name} {version} on crates.io")
            raise SystemExit(0)
    except urllib.error.HTTPError as exc:
        if exc.code not in (404, 429):
            print(f"Unexpected HTTP error while checking {name} {version}: {exc}", file=sys.stderr)
    except Exception as exc:
        print(f"Retrying crates.io visibility check for {name} {version}: {exc}", file=sys.stderr)

    time.sleep(sleep_secs)

print(f"Timed out waiting for {name} {version} to appear on crates.io", file=sys.stderr)
raise SystemExit(1)
PY
}

mkdir -p "$(dirname "$LOG_FILE")"
: > "$LOG_FILE"

echo "Publishing crates in deterministic order..." | tee -a "$LOG_FILE"
if [[ "$DRY_RUN" == "true" ]]; then
  echo "Mode: dry-run" | tee -a "$LOG_FILE"
  echo "Dry-run note: skipping cargo package/publish because crates.io dependency" | tee -a "$LOG_FILE"
  echo "resolution for unpublished intra-workspace versions is expected to fail." | tee -a "$LOG_FILE"
  echo "Running workspace compile check instead." | tee -a "$LOG_FILE"
  cargo check -p tandem-ai -p tandem-tui -p tandem-server -p tandem-core -p tandem-tools -p tandem-memory 2>&1 | tee -a "$LOG_FILE"
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
    echo "DRY-RUN SKIP $crate (publish-time crates.io resolution intentionally not executed)" | tee -a "$LOG_FILE"
    continue
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
      wait_for_crate_version "$crate" 2>&1 | tee -a "$LOG_FILE"
      continue
    fi
    echo "FAIL $crate" | tee -a "$LOG_FILE"
    exit $code
  fi

  echo "OK $crate" | tee -a "$LOG_FILE"
  wait_for_crate_version "$crate" 2>&1 | tee -a "$LOG_FILE"
done

echo "Crates publish flow completed." | tee -a "$LOG_FILE"
