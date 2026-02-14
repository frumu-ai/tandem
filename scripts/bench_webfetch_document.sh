#!/usr/bin/env bash
set -euo pipefail

if [ "${1:-}" = "" ]; then
  echo "Usage: $0 <urls_file> [concurrency] [engine_bin]"
  exit 1
fi

urls_file="$1"
concurrency="${2:-8}"
engine_bin="${3:-}"

if [ ! -f "$urls_file" ]; then
  echo "URLs file not found: $urls_file"
  exit 1
fi

if [ -z "$engine_bin" ]; then
  if [ -x "./target/debug/tandem-engine" ]; then
    engine_bin="./target/debug/tandem-engine"
  elif [ -x "./target/debug/tandem-engine.exe" ]; then
    engine_bin="./target/debug/tandem-engine.exe"
  elif [ -x "./src-tauri/binaries/tandem-engine" ]; then
    engine_bin="./src-tauri/binaries/tandem-engine"
  elif [ -x "./src-tauri/binaries/tandem-engine.exe" ]; then
    engine_bin="./src-tauri/binaries/tandem-engine.exe"
  fi
fi

if [ -z "${engine_bin:-}" ] || [ ! -x "$engine_bin" ]; then
  echo "Engine binary not found. Pass it as the third argument."
  exit 1
fi

time_mode="posix"
time_bin=""
if command -v gtime >/dev/null 2>&1; then
  time_mode="gnu"
  time_bin="gtime"
elif /usr/bin/time -f "%e %M" true >/dev/null 2>&1; then
  time_mode="gnu"
  time_bin="/usr/bin/time"
elif /usr/bin/time -l true >/dev/null 2>&1; then
  time_mode="bsd"
  time_bin="/usr/bin/time"
fi

out_dir="$(mktemp -d)"
export out_dir engine_bin time_mode time_bin

run_line() {
  line="$1"
  idx="${line%%$'\t'*}"
  url="${line#*$'\t'}"
  safe_url="${url//\\/\\\\}"
  safe_url="${safe_url//\"/\\\"}"
  payload="{\"tool\":\"webfetch_document\",\"args\":{\"url\":\"$safe_url\",\"return\":\"text\"}}"
  if [ "$time_mode" = "gnu" ]; then
    { "$time_bin" -f "%e %M" "$engine_bin" tool --json - <<< "$payload" > /dev/null; } 2> "$out_dir/$idx.time"
    read -r elapsed rss_kb < "$out_dir/$idx.time"
  elif [ "$time_mode" = "bsd" ]; then
    { "$time_bin" -l "$engine_bin" tool --json - <<< "$payload" > /dev/null; } 2> "$out_dir/$idx.time"
    elapsed="$(awk '/^real /{print $2}' "$out_dir/$idx.time")"
    rss_bytes="$(awk '/maximum resident set size/{print $1}' "$out_dir/$idx.time")"
    rss_kb="$((rss_bytes / 1024))"
  else
    start="$(date +%s)"
    "$engine_bin" tool --json - <<< "$payload" > /dev/null 2> "$out_dir/$idx.time"
    end="$(date +%s)"
    elapsed="$(awk "BEGIN {print $end-$start}")"
    rss_kb="-1"
  fi
  printf "%s\t%s\t%s\n" "$url" "$elapsed" "$rss_kb" > "$out_dir/$idx.tsv"
}

export -f run_line

grep -v '^[[:space:]]*$' "$urls_file" | nl -ba -w1 -s $'\t' | xargs -P "$concurrency" -n 1 -d '\n' bash -c 'run_line "$0"' 

results="$out_dir/results.tsv"
cat "$out_dir"/*.tsv > "$results"

python - "$results" <<'PY'
import sys, statistics, math
path = sys.argv[1]
rows = []
with open(path, "r", encoding="utf-8") as f:
    for line in f:
        url, elapsed, rss = line.rstrip("\n").split("\t")
        rows.append((url, float(elapsed), int(rss)))

def pctl(values, pct):
    if not values:
        return None
    values = sorted(values)
    k = (len(values) - 1) * pct
    f = math.floor(k)
    c = math.ceil(k)
    if f == c:
        return values[int(k)]
    return values[f] + (values[c] - values[f]) * (k - f)

elapsed = [r[1] for r in rows]
rss = [r[2] for r in rows if r[2] >= 0]

print(f"runs={len(rows)}")
print(f"p50_elapsed_s={pctl(elapsed, 0.50):.3f}")
print(f"p95_elapsed_s={pctl(elapsed, 0.95):.3f}")
if rss:
    print(f"p50_rss_kb={int(pctl(rss, 0.50))}")
    print(f"p95_rss_kb={int(pctl(rss, 0.95))}")
else:
    print("p50_rss_kb=unknown")
    print("p95_rss_kb=unknown")
print(f"results_file={path}")
PY
