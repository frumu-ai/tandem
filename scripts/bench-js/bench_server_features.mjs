import { spawn } from 'child_process';
import { writeFile } from 'fs/promises';
import { dirname, resolve } from 'path';
import { fileURLToPath } from 'url';
import { platform } from 'os';

import pLimit from 'p-limit';

const __filename = fileURLToPath(import.meta.url);
const SCRIPT_DIR = dirname(__filename);
const REPO_ROOT = resolve(SCRIPT_DIR, '..', '..');

const HOST = process.env.BENCH_HOST || '127.0.0.1';
const PORT = Number(process.env.BENCH_PORT || 39731);
const BASE_URL = `http://${HOST}:${PORT}`;

const START_SERVER = (process.env.BENCH_START_SERVER || '0') === '1';
const ENGINE_BIN =
  process.env.TANDEM_BIN ||
  (platform() === 'win32'
    ? resolve(REPO_ROOT, 'target', 'debug', 'tandem-engine.exe')
    : resolve(REPO_ROOT, 'target', 'debug', 'tandem-engine'));

const REQUESTS = Math.max(1, Number(process.env.BENCH_REQUESTS || 200));
const CONCURRENCY = Math.max(1, Number(process.env.BENCH_CONCURRENCY || 20));
const SESSION_LOOPS = Math.max(1, Number(process.env.BENCH_SESSION_LOOPS || 80));
const TOOL_LOOPS = Math.max(1, Number(process.env.BENCH_TOOL_LOOPS || 80));
const ENABLE_SSE = (process.env.BENCH_ENABLE_SSE || '0') === '1';
const SSE_RUNS = Math.max(1, Number(process.env.BENCH_SSE_RUNS || 5));
const TIMEOUT_MS = Math.max(1000, Number(process.env.BENCH_TIMEOUT_MS || 60000));

const TOKEN = (process.env.BENCH_TOKEN || '').trim();

const PROVIDER = (process.env.BENCH_PROVIDER || 'openrouter').trim();
const MODEL = (process.env.BENCH_MODEL || 'openai/gpt-4o-mini').trim();
const API_KEY =
  process.env.BENCH_API_KEY ||
  process.env[`${PROVIDER.toUpperCase()}_API_KEY`] ||
  '';

const REPORT_JSON = process.env.BENCH_FEATURES_REPORT_JSON || resolve(SCRIPT_DIR, 'bench_features_results.json');
const REPORT_TSV = process.env.BENCH_FEATURES_REPORT_TSV || resolve(SCRIPT_DIR, 'bench_features_results.tsv');

function now() {
  return performance.now();
}

function avg(values) {
  if (!values.length) return null;
  return values.reduce((a, b) => a + b, 0) / values.length;
}

function percentile(values, p) {
  if (!values.length) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const idx = Math.min(sorted.length - 1, Math.ceil(sorted.length * p) - 1);
  return sorted[idx];
}

function stats(values) {
  if (!values.length) {
    return { count: 0, avgMs: null, p50Ms: null, p95Ms: null, minMs: null, maxMs: null };
  }
  const sorted = [...values].sort((a, b) => a - b);
  return {
    count: sorted.length,
    avgMs: avg(sorted),
    p50Ms: percentile(sorted, 0.5),
    p95Ms: percentile(sorted, 0.95),
    minMs: sorted[0],
    maxMs: sorted[sorted.length - 1],
  };
}

function fmtMs(v) {
  return v == null ? 'n/a' : `${v.toFixed(1)}ms`;
}

function authHeaders(contentType = true) {
  const headers = {};
  if (contentType) headers['content-type'] = 'application/json';
  if (TOKEN) headers['X-Tandem-Token'] = TOKEN;
  return headers;
}

async function timedFetch(path, opts = {}) {
  const controller = new AbortController();
  const timeout = setTimeout(() => controller.abort(), TIMEOUT_MS);
  const start = now();
  try {
    const res = await fetch(`${BASE_URL}${path}`, { ...opts, signal: controller.signal });
    const elapsedMs = now() - start;
    return { res, elapsedMs };
  } finally {
    clearTimeout(timeout);
  }
}

async function waitReady(timeoutMs = 30000) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    try {
      const { res } = await timedFetch('/global/health', { headers: authHeaders(false) });
      if (res.ok) {
        const json = await res.json();
        if (json?.ready) return true;
      }
    } catch {
      // retry
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  return false;
}

async function benchHealthBurst() {
  const limit = pLimit(CONCURRENCY);
  const latencies = [];
  let failed = 0;
  const started = now();

  const tasks = Array.from({ length: REQUESTS }, () =>
    limit(async () => {
      try {
        const { res, elapsedMs } = await timedFetch('/global/health', { headers: authHeaders(false) });
        if (!res.ok) {
          failed += 1;
          return;
        }
        latencies.push(elapsedMs);
      } catch {
        failed += 1;
      }
    })
  );

  await Promise.all(tasks);
  const wallMs = now() - started;
  const s = stats(latencies);
  return {
    scenario: 'health_burst',
    requests: REQUESTS,
    concurrency: CONCURRENCY,
    failures: failed,
    wallMs,
    rps: REQUESTS / (wallMs / 1000),
    ...s,
  };
}

async function benchSessionLifecycle() {
  const limit = pLimit(CONCURRENCY);
  const latencies = [];
  let failed = 0;
  const started = now();

  const tasks = Array.from({ length: SESSION_LOOPS }, (_, i) =>
    limit(async () => {
      const start = now();
      try {
        const create = await timedFetch('/session', {
          method: 'POST',
          headers: authHeaders(true),
          body: JSON.stringify({ title: `bench-session-${i}`, directory: '.' }),
        });
        if (!create.res.ok) {
          failed += 1;
          return;
        }
        const session = await create.res.json();
        const sid = session?.id;
        if (!sid) {
          failed += 1;
          return;
        }

        const getOne = await timedFetch(`/session/${sid}`, { headers: authHeaders(false) });
        if (!getOne.res.ok) {
          failed += 1;
          return;
        }

        const del = await timedFetch(`/session/${sid}`, {
          method: 'DELETE',
          headers: authHeaders(false),
        });
        if (!del.res.ok) {
          failed += 1;
          return;
        }
        latencies.push(now() - start);
      } catch {
        failed += 1;
      }
    })
  );

  await Promise.all(tasks);
  const wallMs = now() - started;
  const s = stats(latencies);
  return {
    scenario: 'session_lifecycle',
    loops: SESSION_LOOPS,
    concurrency: CONCURRENCY,
    failures: failed,
    wallMs,
    opsPerSec: SESSION_LOOPS / (wallMs / 1000),
    ...s,
  };
}

async function benchToolExecute() {
  const limit = pLimit(CONCURRENCY);
  const latencies = [];
  let failed = 0;
  const started = now();

  const tasks = Array.from({ length: TOOL_LOOPS }, (_, i) =>
    limit(async () => {
      try {
        const { res, elapsedMs } = await timedFetch('/tool/execute', {
          method: 'POST',
          headers: authHeaders(true),
          body: JSON.stringify({
            tool: 'bash',
            args: { command: `echo bench_${i}` },
          }),
        });
        if (!res.ok) {
          failed += 1;
          return;
        }
        const json = await res.json();
        if (json?.ok === false) {
          failed += 1;
          return;
        }
        latencies.push(elapsedMs);
      } catch {
        failed += 1;
      }
    })
  );

  await Promise.all(tasks);
  const wallMs = now() - started;
  const s = stats(latencies);
  return {
    scenario: 'tool_execute_bash',
    loops: TOOL_LOOPS,
    concurrency: CONCURRENCY,
    failures: failed,
    wallMs,
    opsPerSec: TOOL_LOOPS / (wallMs / 1000),
    ...s,
  };
}

async function readSseUntilFinished(url) {
  const startConnect = now();
  const res = await fetch(url, {
    headers: { ...authHeaders(false), Accept: 'text/event-stream' },
  });
  if (!res.ok || !res.body) {
    const body = await res.text();
    throw new Error(`SSE stream failed (${res.status}): ${body}`);
  }

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';
  let firstEventMs = null;

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;
    buffer += decoder.decode(value, { stream: true });
    if (firstEventMs == null) firstEventMs = now() - startConnect;

    const parts = buffer.replace(/\r\n/g, '\n').split('\n\n');
    buffer = parts.pop() || '';
    for (const part of parts) {
      const lines = part
        .split('\n')
        .filter((line) => line.startsWith('data:'))
        .map((line) => line.slice(5).trim());
      if (!lines.length) continue;
      const raw = lines.join('\n');
      let evt = null;
      try {
        evt = JSON.parse(raw);
      } catch {
        continue;
      }
      if (evt?.type === 'session.run.finished') {
        try {
          await reader.cancel();
        } catch {
          // no-op
        }
        return {
          firstEventMs: firstEventMs ?? now() - startConnect,
          totalStreamMs: now() - startConnect,
          status: evt?.properties?.status || 'unknown',
        };
      }
    }
  }

  return {
    firstEventMs: firstEventMs ?? now() - startConnect,
    totalStreamMs: now() - startConnect,
    status: 'stream_closed',
  };
}

async function benchSsePromptAsync() {
  if (!API_KEY) {
    return {
      scenario: 'sse_prompt_async',
      skipped: true,
      reason: 'BENCH_API_KEY (or provider env key) not set',
    };
  }

  const ttfb = [];
  const totals = [];
  let failed = 0;

  for (let i = 0; i < SSE_RUNS; i += 1) {
    try {
      const create = await timedFetch('/session', {
        method: 'POST',
        headers: authHeaders(true),
        body: JSON.stringify({
          title: `bench-sse-${i}`,
          directory: '.',
          provider: PROVIDER,
          model: {
            provider_id: PROVIDER,
            model_id: MODEL,
          },
        }),
      });
      if (!create.res.ok) {
        failed += 1;
        continue;
      }
      const session = await create.res.json();
      const sid = session?.id;
      if (!sid) {
        failed += 1;
        continue;
      }

      const prompt = {
        parts: [{ type: 'text', text: 'Reply with exactly: ok' }],
        model: {
          provider_id: PROVIDER,
          model_id: MODEL,
        },
      };
      const runStart = await timedFetch(`/session/${sid}/prompt_async?return=run`, {
        method: 'POST',
        headers: authHeaders(true),
        body: JSON.stringify(prompt),
      });
      if (!runStart.res.ok) {
        failed += 1;
        continue;
      }
      const run = await runStart.res.json();
      const attach = run?.attachEventStream;
      if (!attach) {
        failed += 1;
        continue;
      }
      const sse = await readSseUntilFinished(`${BASE_URL}${attach}`);
      if (sse.status !== 'completed') {
        failed += 1;
        continue;
      }
      ttfb.push(sse.firstEventMs);
      totals.push(sse.totalStreamMs);
    } catch {
      failed += 1;
    }
  }

  return {
    scenario: 'sse_prompt_async',
    runs: SSE_RUNS,
    failures: failed,
    firstEvent: stats(ttfb),
    streamTotal: stats(totals),
  };
}

async function run() {
  console.log('Tandem Server Feature Benchmark');
  console.log(`base=${BASE_URL}`);
  console.log(`startServer=${START_SERVER} engineBin=${ENGINE_BIN}`);
  console.log(`tokenHeader=${TOKEN ? 'enabled' : 'disabled'}`);

  let server = null;
  if (START_SERVER) {
    const args = ['serve', '--host', HOST, '--port', String(PORT)];
    if (PROVIDER) args.push('--provider', PROVIDER);
    if (MODEL) args.push('--model', MODEL);
    if (API_KEY) args.push('--api-key', API_KEY);
    if (TOKEN) args.push('--api-token', TOKEN);

    server = spawn(ENGINE_BIN, args, {
      stdio: ['ignore', 'pipe', 'pipe'],
      shell: false,
      env: { ...process.env },
    });
    server.stdout.on('data', (d) => process.stdout.write(`[bench-serve] ${d}`));
    server.stderr.on('data', (d) => process.stderr.write(`[bench-serve] ${d}`));
  }

  try {
    const ready = await waitReady();
    if (!ready) throw new Error(`server not ready at ${BASE_URL}`);

    const results = [];
    results.push(await benchHealthBurst());
    results.push(await benchSessionLifecycle());
    results.push(await benchToolExecute());
    if (ENABLE_SSE) {
      results.push(await benchSsePromptAsync());
    }

    const summary = {
      timestamp: new Date().toISOString(),
      config: {
        baseUrl: BASE_URL,
        requests: REQUESTS,
        concurrency: CONCURRENCY,
        sessionLoops: SESSION_LOOPS,
        toolLoops: TOOL_LOOPS,
        enableSse: ENABLE_SSE,
        sseRuns: SSE_RUNS,
        provider: PROVIDER,
        model: MODEL,
        apiKeySet: Boolean(API_KEY),
        tokenSet: Boolean(TOKEN),
      },
      results,
    };

    await writeFile(REPORT_JSON, JSON.stringify(summary, null, 2), 'utf-8');
    const tsvRows = ['scenario\tcount\tfailures\tavg_ms\tp50_ms\tp95_ms'];
    for (const r of results) {
      if (r.skipped) {
        tsvRows.push(`${r.scenario}\t0\t0\t\t\t`);
        continue;
      }
      if (r.scenario === 'sse_prompt_async') {
        tsvRows.push(
          `${r.scenario}_first_event\t${r.firstEvent?.count || 0}\t${r.failures || 0}\t${r.firstEvent?.avgMs ?? ''}\t${r.firstEvent?.p50Ms ?? ''}\t${r.firstEvent?.p95Ms ?? ''}`
        );
        tsvRows.push(
          `${r.scenario}_total_stream\t${r.streamTotal?.count || 0}\t${r.failures || 0}\t${r.streamTotal?.avgMs ?? ''}\t${r.streamTotal?.p50Ms ?? ''}\t${r.streamTotal?.p95Ms ?? ''}`
        );
        continue;
      }
      tsvRows.push(
        `${r.scenario}\t${r.count || 0}\t${r.failures || 0}\t${r.avgMs ?? ''}\t${r.p50Ms ?? ''}\t${r.p95Ms ?? ''}`
      );
    }
    await writeFile(REPORT_TSV, `${tsvRows.join('\n')}\n`, 'utf-8');

    console.log('\n=== Server Feature Benchmark Summary ===');
    for (const r of results) {
      if (r.skipped) {
        console.log(`${r.scenario}: skipped (${r.reason})`);
        continue;
      }
      if (r.scenario === 'sse_prompt_async') {
        console.log(
          `${r.scenario}: runs=${r.runs} fail=${r.failures} first_event p50=${fmtMs(
            r.firstEvent?.p50Ms
          )} total p50=${fmtMs(r.streamTotal?.p50Ms)}`
        );
        continue;
      }
      console.log(
        `${r.scenario}: count=${r.count} fail=${r.failures} avg=${fmtMs(r.avgMs)} p50=${fmtMs(
          r.p50Ms
        )} p95=${fmtMs(r.p95Ms)}`
      );
    }
    console.log(`Reports:\n- ${REPORT_JSON}\n- ${REPORT_TSV}`);
  } finally {
    if (server && !server.killed) {
      server.kill();
    }
  }
}

run().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});

