#!/usr/bin/env bash
set -euo pipefail

HOSTNAME="${HOSTNAME:-127.0.0.1}"
PORT="${PORT:-39731}"
STATE_DIR="${STATE_DIR:-.tandem-smoke}"
OUT_DIR="${OUT_DIR:-runtime-proof}"
HEALTH_TIMEOUT_SECONDS="${HEALTH_TIMEOUT_SECONDS:-30}"

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_PATH="$ROOT_DIR/$OUT_DIR"
STATE_PATH="$ROOT_DIR/$STATE_DIR"

mkdir -p "$OUT_PATH" "$STATE_PATH"
rm -f "$OUT_PATH"/*

pkill -f tandem-engine >/dev/null 2>&1 || true

cleanup() {
  if [[ -n "${SSE_PID:-}" ]]; then
    kill "$SSE_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "${ENGINE_PID:-}" ]]; then
    kill "$ENGINE_PID" >/dev/null 2>&1 || true
  fi
  pkill -f tandem-engine >/dev/null 2>&1 || true
}
trap cleanup EXIT

cd "$ROOT_DIR"
cargo build -p tandem-ai

"$ROOT_DIR/target/debug/tandem-engine" serve \
  --host "$HOSTNAME" \
  --port "$PORT" \
  --state-dir "$STATE_PATH" \
  >"$OUT_PATH/serve.stdout.log" \
  2>"$OUT_PATH/serve.stderr.log" &
ENGINE_PID=$!

for ((i = 0; i < HEALTH_TIMEOUT_SECONDS * 2; i++)); do
  if curl -sf "http://$HOSTNAME:$PORT/global/health" >"$OUT_PATH/health.json"; then
    break
  fi
  sleep 0.5
done

if ! [[ -s "$OUT_PATH/health.json" ]]; then
  echo "Engine did not become healthy"
  exit 1
fi

curl -sf -X POST "http://$HOSTNAME:$PORT/session" \
  -H "content-type: application/json" \
  -d "{}" >"$OUT_PATH/session.create.json"
SID="$(jq -r '.id' "$OUT_PATH/session.create.json")"
if [[ -z "$SID" || "$SID" == "null" ]]; then
  echo "Failed to create session"
  exit 1
fi
printf '%s\n' "$SID" >"$OUT_PATH/session.id.txt"

curl -sf "http://$HOSTNAME:$PORT/session" >"$OUT_PATH/session.list.json"
curl -sf -X POST "http://$HOSTNAME:$PORT/session/$SID/message" \
  -H "content-type: application/json" \
  -d '{"parts":[{"type":"text","text":"message for smoke test"}]}' \
  >"$OUT_PATH/session.post_message.json"
curl -sf "http://$HOSTNAME:$PORT/session/$SID/message" >"$OUT_PATH/session.messages.json"
curl -sf "http://$HOSTNAME:$PORT/provider" >"$OUT_PATH/provider.list.json"

curl -N -s "http://$HOSTNAME:$PORT/event" >"$OUT_PATH/event.log" &
SSE_PID=$!
sleep 1
curl -sf -X POST "http://$HOSTNAME:$PORT/session/$SID/prompt_async" \
  -H "content-type: application/json" \
  -d '{"parts":[{"type":"text","text":"hello streaming"}]}' >/dev/null

for _ in {1..20}; do
  if grep -q "message.part.updated" "$OUT_PATH/event.log"; then
    grep -m1 "message.part.updated" "$OUT_PATH/event.log" >"$OUT_PATH/sse.message.part.updated.line.txt"
    break
  fi
  sleep 0.5
done

if ! [[ -s "$OUT_PATH/sse.message.part.updated.line.txt" ]]; then
  echo "Did not capture message.part.updated in SSE stream"
  exit 1
fi

sleep 60
IDLE_RSS_KB="$(ps -o rss= -p "$ENGINE_PID" | awk '{print $1}')"
printf '{"rss_kb": %s}\n' "${IDLE_RSS_KB:-0}" >"$OUT_PATH/memory.idle.json"

curl -sf -X POST "http://$HOSTNAME:$PORT/session/$SID/prompt_async" \
  -H "content-type: application/json" \
  -d '{"parts":[{"type":"text","text":"/tool todo_write {\"todos\":[{\"content\":\"runtime proof todo\"}]}"}]}' \
  >/dev/null

PERM_ID="$(curl -sf "http://$HOSTNAME:$PORT/permission" | jq -r '[.[] | select(.status=="pending")][0].id // empty')"
if [[ -n "$PERM_ID" ]]; then
  curl -sf -X POST "http://$HOSTNAME:$PORT/permission/$PERM_ID/reply" \
    -H "content-type: application/json" \
    -d '{"reply":"allow"}' \
    >"$OUT_PATH/permission.reply.json"
fi

peak_kb=0
printf '[\n' >"$OUT_PATH/memory.samples.json"
for i in {1..15}; do
  rss_kb="$(ps -o rss= -p "$ENGINE_PID" | awk '{print $1}')"
  if [[ -z "$rss_kb" ]]; then
    rss_kb=0
  fi
  if ((rss_kb > peak_kb)); then
    peak_kb="$rss_kb"
  fi
  printf '  {"sample": %d, "rss_kb": %s}%s\n' "$i" "$rss_kb" "$( [[ "$i" -lt 15 ]] && echo "," )" >>"$OUT_PATH/memory.samples.json"
  sleep 2
done
printf ']\n' >>"$OUT_PATH/memory.samples.json"
printf '{"rss_kb": %s}\n' "$peak_kb" >"$OUT_PATH/memory.peak.json"

echo "Smoke test PASS. Artifacts in $OUT_PATH"
