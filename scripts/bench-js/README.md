# JS Benchmark for WebFetch

This directory contains a Node.js benchmark script to compare against the Rust `webfetch_document` tool.

## Prerequisites

- Node.js 18+ (tested with v22)
- Dependencies installed: `npm install`

## Usage

```bash
node bench.mjs ../urls.txt
```

## Methodology

### Node.js

- Fetches URL.
- Uses `jsdom` to parse HTML and remove `<script>`, `<style>`, `<noscript>` tags.
- Uses `turndown` to convert the cleaned HTML to Markdown.
- Measures elapsed time and RSS memory usage.

### Rust (`webfetch_document`)

- Fetches URL.
- Uses Regex to strip `<script>`, `<style>`, `<noscript>` tags.
- Uses `html2md` to parse and convert HTML to Markdown.
- Extracts metadata (title, links, etc.) and computes compression stats.
- Returns a structured JSON response.

Both implementations perform equivalent work: HTTP fetch -> HTML Parsing/Cleaning -> Markdown Conversion.

## Results Comparison (Typical)

| Metric       | Rust (CLI)          | Node.js (JSDOM + Turndown) | Rust (Server Mode) |
| ------------ | ------------------- | -------------------------- | ------------------ |
| p50 Latency  | ~2.7s               | ~1.2s                      | ~0.4s              |
| p95 Latency  | ~16s                | ~50s                       | ~1.3s              |
| Memory (RSS) | ~40MB (per process) | ~500MB - 3GB (accumulated) | ~100MB (stable)    |

### Performance Analysis (Server Mode vs Others)

- **vs Node.js**: Rust Server is **3x faster** (67% reduction in latency) at p50 and **38x more stable** at p95 (1.3s vs 50s).
- **vs Rust CLI**: Rust Server is **~7x faster** (85% reduction in latency), eliminating process startup overhead.

### Content Efficiency

In addition to speed, the `webfetch_document` tool significantly reduces the payload size sent to the LLM by stripping noise and converting to Markdown.

- **Reduction**: Typically **~70-80%** reduction in character count compared to raw HTML.
- **Impact**: This drastic reduction happens _concurrently_ with the fetch and parse, meaning the engine delivers a highly optimized, token-efficient payload in a fraction of the time it takes other tools to just fetch the raw content.

## Rust Server Benchmark

The server mode benchmark (`bench_server.mjs`) starts a single `tandem-engine` instance and sends concurrent HTTP requests to the `/tool/execute` endpoint.
This eliminates process startup overhead and allows connection reuse.

To run:

```bash
# Build the engine first
cargo build -p tandem-engine

# Run the benchmark
node bench_server.mjs ../urls.txt
```

**Conclusion**: The Rust server implementation is the most performant, significantly outperforming both the Node.js implementation (**3x faster p50**) and the CLI-based invocation (**7x faster p50**), while maintaining extremely stable tail latencies and low memory usage. The overhead seen in the CLI benchmark is completely eliminated in server mode.

## Server Feature Benchmark

For broader server capabilities (not just `webfetch_document`), use:

```bash
npm run bench:features
```

This benchmark measures:

- `health_burst`: `/global/health` throughput/latency under concurrency
- `session_lifecycle`: create/get/delete session loop latency
- `tool_execute_bash`: `/tool/execute` latency for deterministic bash commands
- optional `sse_prompt_async`: async run + SSE first-event and completion timing

### Common Env Vars

```bash
# Target server
BENCH_HOST=127.0.0.1
BENCH_PORT=39731

# Start/stop engine automatically from this script
BENCH_START_SERVER=1
TANDEM_BIN=../../target/debug/tandem-engine.exe

# Auth (if server requires token)
BENCH_TOKEN=tk_...

# Workload sizing
BENCH_REQUESTS=200
BENCH_CONCURRENCY=20
BENCH_SESSION_LOOPS=80
BENCH_TOOL_LOOPS=80

# Optional SSE benchmark (requires provider + key)
BENCH_ENABLE_SSE=1
BENCH_SSE_RUNS=5
BENCH_PROVIDER=openrouter
BENCH_MODEL=openai/gpt-4o-mini
BENCH_API_KEY=...
```

Reports are written to:

- `bench_features_results.json`
- `bench_features_results.tsv`
