#!/usr/bin/env bash
set -euo pipefail

# Measures process-cold automation startup by restarting engine each trial and timing:
# boot readiness, run_now ACK, and run record visibility.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"

BASE_URL="${TANDEM_BASE_URL:-http://127.0.0.1:39731}"
API_FAMILY="${TANDEM_AUTOMATION_API:-routines}"
RUNS="${BENCH_RUNS:-5}"
STARTUP_TIMEOUT_SECONDS="${BENCH_STARTUP_TIMEOUT_SECONDS:-45}"
RUN_VISIBLE_TIMEOUT_SECONDS="${BENCH_RUN_VISIBLE_TIMEOUT_SECONDS:-20}"
POLL_MS="${BENCH_POLL_MS:-200}"

HOST="$(echo "$BASE_URL" | sed -E 's#https?://([^:/]+).*#\1#')"
PORT="$(echo "$BASE_URL" | sed -E 's#https?://[^:/]+:([0-9]+).*#\1#')"
if [[ "$PORT" == "$BASE_URL" ]]; then
  PORT="39731"
fi

ENGINE_CMD="${TANDEM_ENGINE_CMD:-$REPO_ROOT/target/debug/tandem-engine serve --host $HOST --port $PORT}"

if ! command -v jq >/dev/null 2>&1; then
  echo "jq is required for benchmark_cold_start.sh"
  exit 1
fi

if [[ "$API_FAMILY" == "automations" ]]; then
  CREATE_PATH="/automations"
  RUN_NOW_PREFIX="/automations"
  RUN_PATH_PREFIX="/automations/runs"
else
  CREATE_PATH="/routines"
  RUN_NOW_PREFIX="/routines"
  RUN_PATH_PREFIX="/routines/runs"
fi

wait_ready() {
  local timeout="$1"
  local poll_ms="$2"
  local deadline
  deadline=$((SECONDS + timeout))
  while (( SECONDS < deadline )); do
    if curl -fsS "$BASE_URL/global/health" | jq -e '.ready == true' >/dev/null 2>&1; then
      return 0
    fi
    sleep "$(awk "BEGIN { printf \"%.3f\", $poll_ms/1000 }")"
  done
  return 1
}

percentile() {
  local file="$1"
  local p="$2"
  awk -v p="$p" '
    { a[NR]=$1 }
    END {
      if (NR==0) { print "nan"; exit 0 }
      n=asort(a)
      idx=int((n*p)+0.999999)-1
      if (idx < 0) idx=0
      if (idx >= n) idx=n-1
      print a[idx+1]
    }
  ' "$file"
}

tmp_boot="$(mktemp)"
tmp_ack="$(mktemp)"
tmp_visible="$(mktemp)"
tmp_total="$(mktemp)"
out_json="$SCRIPT_DIR/cold_start_results.json"
bench_id="bench-cold-start-$(date +%s)"

cleanup() {
  rm -f "$tmp_boot" "$tmp_ack" "$tmp_visible" "$tmp_total"
}
trap cleanup EXIT

echo "== Ensure benchmark routine exists =="
curl -sS -X POST "$BASE_URL$CREATE_PATH" \
  -H "content-type: application/json" \
  -d "{
    \"routine_id\":\"$bench_id\",
    \"name\":\"Cold Start Benchmark Routine\",
    \"schedule\":{\"interval_seconds\":{\"seconds\":3600}},
    \"entrypoint\":\"mission.default\",
    \"allowed_tools\":[\"read\"],
    \"requires_approval\":false,
    \"external_integrations_allowed\":false
  }" >/dev/null

echo "{ \"timestamp\": \"$(date -u +%Y-%m-%dT%H:%M:%SZ)\", \"base_url\": \"$BASE_URL\", \"api_family\": \"$API_FAMILY\", \"runs\": $RUNS, \"results\": [" > "$out_json"

for i in $(seq 1 "$RUNS"); do
  echo
  echo "== Trial $i/$RUNS =="

  engine_start_ms="$(python - <<'PY'
import time
print(int(time.time()*1000))
PY
)"

  bash -lc "$ENGINE_CMD" >/tmp/tandem-bench-engine.log 2>&1 &
  engine_pid=$!

  if ! wait_ready "$STARTUP_TIMEOUT_SECONDS" "$POLL_MS"; then
    kill "$engine_pid" >/dev/null 2>&1 || true
    echo "engine did not become ready within timeout"
    exit 1
  fi

  engine_ready_ms="$(python - <<'PY'
import time
print(int(time.time()*1000))
PY
)"
  engine_boot_ms=$((engine_ready_ms - engine_start_ms))

  # API-side enqueue/ack latency for mission-trigger path.
  run_now_start_ms="$(python - <<'PY'
import time
print(int(time.time()*1000))
PY
)"
  run_now_resp="$(curl -sS -X POST "$BASE_URL$RUN_NOW_PREFIX/$bench_id/run_now" -H "content-type: application/json" -d '{}')"
  run_now_end_ms="$(python - <<'PY'
import time
print(int(time.time()*1000))
PY
)"
  run_now_ack_ms=$((run_now_end_ms - run_now_start_ms))

  run_id="$(echo "$run_now_resp" | jq -r '.runID // .runId // .run_id // .id // .run.runID // .run.runId // .run.run_id // .run.id // empty')"
  if [[ -z "$run_id" ]]; then
    kill "$engine_pid" >/dev/null 2>&1 || true
    echo "could not parse run id"
    exit 1
  fi

  # End-to-end mission trigger visibility gate (run record exists).
  visible_start_ms="$run_now_start_ms"
  visible_deadline=$((SECONDS + RUN_VISIBLE_TIMEOUT_SECONDS))
  visible_ok=0
  while (( SECONDS < visible_deadline )); do
    if curl -fsS "$BASE_URL$RUN_PATH_PREFIX/$run_id" >/dev/null 2>&1; then
      visible_ok=1
      break
    fi
    sleep "$(awk "BEGIN { printf \"%.3f\", $POLL_MS/1000 }")"
  done
  if [[ "$visible_ok" -ne 1 ]]; then
    kill "$engine_pid" >/dev/null 2>&1 || true
    echo "run record not visible within timeout"
    exit 1
  fi

  visible_end_ms="$(python - <<'PY'
import time
print(int(time.time()*1000))
PY
)"
  run_visible_ms=$((visible_end_ms - visible_start_ms))
  total_ms=$((engine_boot_ms + run_visible_ms))

  echo "$engine_boot_ms" >> "$tmp_boot"
  echo "$run_now_ack_ms" >> "$tmp_ack"
  echo "$run_visible_ms" >> "$tmp_visible"
  echo "$total_ms" >> "$tmp_total"

  echo "engine_boot_ms=$engine_boot_ms run_now_ack_ms=$run_now_ack_ms run_visible_ms=$run_visible_ms total_ms=$total_ms"

  if [[ "$i" -gt 1 ]]; then
    echo "," >> "$out_json"
  fi
  echo "  {\"trial\":$i,\"engine_boot_ms\":$engine_boot_ms,\"run_now_ack_ms\":$run_now_ack_ms,\"run_visible_ms\":$run_visible_ms,\"cold_start_to_run_visible_ms\":$total_ms,\"run_id\":\"$run_id\"}" >> "$out_json"

  kill "$engine_pid" >/dev/null 2>&1 || true
  wait "$engine_pid" 2>/dev/null || true
done

echo "] }" >> "$out_json"

boot_p50="$(percentile "$tmp_boot" 0.5)"
boot_p95="$(percentile "$tmp_boot" 0.95)"
ack_p50="$(percentile "$tmp_ack" 0.5)"
ack_p95="$(percentile "$tmp_ack" 0.95)"
visible_p50="$(percentile "$tmp_visible" 0.5)"
visible_p95="$(percentile "$tmp_visible" 0.95)"
total_p50="$(percentile "$tmp_total" 0.5)"
total_p95="$(percentile "$tmp_total" 0.95)"

echo
echo "== Summary =="
echo "engine_boot_ms   p50=$boot_p50 p95=$boot_p95"
echo "run_now_ack_ms   p50=$ack_p50 p95=$ack_p95"
echo "run_visible_ms   p50=$visible_p50 p95=$visible_p95"
echo "cold_start_total p50=$total_p50 p95=$total_p95"
echo
echo "Saved: $out_json"
