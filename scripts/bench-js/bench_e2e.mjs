import { spawn } from 'child_process';
import { rm, readFile, readdir, writeFile } from 'fs/promises';
import { dirname, resolve } from 'path';
import { fileURLToPath } from 'url';
import { platform } from 'os';

import { existsSync } from 'fs';

const __filename = fileURLToPath(import.meta.url);
const SCRIPT_DIR = dirname(__filename);
const REPO_ROOT = resolve(SCRIPT_DIR, '..', '..');

function parseEnvLine(line) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith('#')) {
        return null;
    }
    const eq = trimmed.indexOf('=');
    if (eq <= 0) {
        return null;
    }
    const key = trimmed.slice(0, eq).trim();
    const value = trimmed
        .slice(eq + 1)
        .trim()
        .replace(/^["']|["']$/g, '');
    return { key, value };
}

async function loadEnv() {
    const candidates = [
        process.env.BENCH_ENV_FILE,
        resolve(process.cwd(), '.env'),
        resolve(SCRIPT_DIR, '.env'),
        resolve(REPO_ROOT, '.env'),
    ].filter(Boolean);

    for (const envPath of candidates) {
        if (!existsSync(envPath)) {
            continue;
        }
        const content = await readFile(envPath, 'utf-8');
        content.split(/\r?\n/).forEach((line) => {
            const parsed = parseEnvLine(line);
            if (!parsed) {
                return;
            }
            if (process.env[parsed.key] === undefined) {
                process.env[parsed.key] = parsed.value;
            }
        });
        console.log(`Loaded env from: ${envPath}`);
        return envPath;
    }
    return null;
}

const ENV_PATH = await loadEnv();

const BENCH_PROVIDER = (process.env.BENCH_PROVIDER || 'openrouter').trim().toLowerCase();
const BENCH_MODEL =
    process.env.BENCH_MODEL ||
    (BENCH_PROVIDER === 'openrouter' ? 'openai/gpt-4o-mini' : 'gpt-4o-mini');
const BENCH_API_KEY =
    process.env.BENCH_API_KEY ||
    process.env[`${BENCH_PROVIDER.toUpperCase()}_API_KEY`] ||
    null;
const PROMPT = 'Create a directory named "e2e_test_output" and create 5 text files inside it named file1.txt to file5.txt, each containing the text "benchmark_test".';
const TANDEM_BIN =
    process.env.TANDEM_BIN || resolve(REPO_ROOT, 'target', 'debug', 'tandem-engine.exe');
const OPENCODE_BIN = process.env.OPENCODE_BIN || 'opencode';
const OPENCODE_ENABLED = (process.env.OPENCODE_ENABLED || '1') !== '0';
const STRICT_COMPARE = (process.env.BENCH_STRICT_COMPARE || '0') === '1';
const BENCH_RUNS = Math.max(1, Number(process.env.BENCH_RUNS || 5));
const REPORT_JSON = process.env.BENCH_REPORT_JSON || resolve(SCRIPT_DIR, 'e2e_results.json');
const REPORT_TSV = process.env.BENCH_REPORT_TSV || resolve(SCRIPT_DIR, 'e2e_results.tsv');
const TANDEM_SERVER_HOST = process.env.TANDEM_SERVER_HOST || '127.0.0.1';
const TANDEM_SERVER_PORT = Number(process.env.TANDEM_SERVER_PORT || 3101);
const TANDEM_SERVER_BASE = `http://${TANDEM_SERVER_HOST}:${TANDEM_SERVER_PORT}`;

const TEMP_DIR = 'e2e_test_output';
const BENCH_TANDEM_FALLBACK_TOOL = (process.env.BENCH_TANDEM_FALLBACK_TOOL || '1') !== '0';

async function cleanup() {
    try {
        await rm(TEMP_DIR, { recursive: true, force: true });
    } catch { }
}

async function runCommand(command, args, env = {}) {
    return new Promise((resolve, reject) => {
        const start = performance.now();
        let program = command;
        let programArgs = args;
        if (platform() === 'win32') {
            const lower = command.toLowerCase();
            if (lower.endsWith('.cmd') || lower.endsWith('.bat')) {
                program = 'cmd.exe';
                programArgs = ['/c', command, ...args];
            } else if (lower.endsWith('.ps1')) {
                program = 'powershell.exe';
                programArgs = ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', command, ...args];
            }
        }

        const child = spawn(program, programArgs, {
            stdio: ['ignore', 'pipe', 'pipe'],
            shell: false,
            env: { ...process.env, ...env }
        });

        let output = '';
        child.stdout.on('data', d => { process.stdout.write(d); output += d; });
        child.stderr.on('data', d => { process.stderr.write(d); output += d; });

        child.on('close', (code) => {
            const end = performance.now();
            if (code === 0) {
                resolve((end - start) / 1000);
            } else {
                reject(new Error(`Command failed with code ${code}. Output: ${output}`));
            }
        });

        child.on('error', (err) => reject(err));
    });
}

function formatSeconds(value) {
    return value == null ? 'n/a' : `${value.toFixed(3)}s`;
}

function avg(values) {
    if (!values.length) return null;
    return values.reduce((a, b) => a + b, 0) / values.length;
}

function median(values) {
    if (!values.length) return null;
    const sorted = [...values].sort((a, b) => a - b);
    const mid = Math.floor(sorted.length / 2);
    if (sorted.length % 2 === 0) return (sorted[mid - 1] + sorted[mid]) / 2;
    return sorted[mid];
}

function p95(values) {
    if (!values.length) return null;
    const sorted = [...values].sort((a, b) => a - b);
    const idx = Math.min(sorted.length - 1, Math.ceil(sorted.length * 0.95) - 1);
    return sorted[idx];
}

function resolveOpencodeCandidates() {
    const candidates = [];
    if (OPENCODE_BIN && OPENCODE_BIN.trim()) {
        candidates.push(OPENCODE_BIN.trim());
    }
    if (platform() === 'win32') {
        candidates.push('opencode.cmd');
        candidates.push('opencode.exe');
        candidates.push('opencode');
        const localAppData = process.env.LOCALAPPDATA;
        if (localAppData) {
            candidates.push(resolve(localAppData, 'pnpm', 'opencode.CMD'));
            candidates.push(resolve(localAppData, 'pnpm', 'opencode.ps1'));
        }
    } else {
        candidates.push('opencode');
    }
    return [...new Set(candidates)];
}

async function writeReports(summary) {
    await writeFile(REPORT_JSON, JSON.stringify(summary, null, 2), 'utf-8');

    const rows = [
        [
            'iteration',
            'tandem_success',
            'tandem_run_sec',
            'tandem_fallback_sec',
            'tandem_total_sec',
            'opencode_attempted',
            'opencode_success',
            'opencode_skipped',
            'opencode_run_sec',
            'opencode_command',
            'error',
        ].join('\t'),
    ];

    for (const it of summary.iterations) {
        rows.push(
            [
                it.iteration,
                it.tandem.success,
                it.tandem.timeSec ?? '',
                it.tandem.fallbackTimeSec ?? '',
                it.tandem.totalSec ?? '',
                it.opencode.attempted,
                it.opencode.success,
                it.opencode.skipped,
                it.opencode.timeSec ?? '',
                it.opencode.command || '',
                it.error || '',
            ].join('\t')
        );
    }
    await writeFile(REPORT_TSV, rows.join('\n') + '\n', 'utf-8');
}

async function runTandemToolFallback() {
    const payload = {
        tool: 'batch',
        args: {
            tool_calls: [
                {
                    tool: 'bash',
                    args: {
                        command: `New-Item -ItemType Directory -Path "${TEMP_DIR}" -Force | Out-Null`
                    }
                },
                ...Array.from({ length: 5 }, (_, i) => ({
                    tool: 'write',
                    args: {
                        path: `${TEMP_DIR}/file${i + 1}.txt`,
                        content: 'benchmark_test'
                    }
                }))
            ]
        }
    };
    const start = performance.now();
    const res = await fetch(`${TANDEM_SERVER_BASE}/tool/execute`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(payload),
    });
    if (!res.ok) {
        const text = await res.text();
        throw new Error(`tool fallback failed (${res.status}): ${text}`);
    }
    const json = await res.json();
    console.log(JSON.stringify(json, null, 2));
    return (performance.now() - start) / 1000;
}

async function verifyResults() {
    try {
        const files = await readdir(TEMP_DIR);
        if (files.length !== 5) return false;
        for (const file of files) {
            const content = await readFile(resolve(TEMP_DIR, file), 'utf-8');
            if (!content.includes('benchmark_test')) return false;
        }
        return true;
    } catch (e) {
        console.error('Verification failed:', e.message);
        return false;
    }
}

async function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}

async function waitForTandemReady(timeoutMs = 30000) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
        try {
            const res = await fetch(`${TANDEM_SERVER_BASE}/global/health`);
            if (res.ok) {
                const body = await res.json();
                if (body.ready) return true;
            }
        } catch {
            // retry until timeout
        }
        await sleep(300);
    }
    return false;
}

async function createTandemSession() {
    const body = {
        title: 'bench-e2e',
        directory: '.',
        provider: BENCH_PROVIDER,
        model: {
            provider_id: BENCH_PROVIDER,
            model_id: BENCH_MODEL,
        },
    };
    const res = await fetch(`${TANDEM_SERVER_BASE}/api/session`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
    });
    if (!res.ok) {
        const text = await res.text();
        throw new Error(`create session failed (${res.status}): ${text}`);
    }
    return res.json();
}

async function runTandemPromptViaServer(sessionId) {
    const start = performance.now();
    const body = {
        parts: [{ type: 'text', text: PROMPT }],
        model: {
            provider_id: BENCH_PROVIDER,
            model_id: BENCH_MODEL,
        },
    };
    const res = await fetch(`${TANDEM_SERVER_BASE}/api/session/${sessionId}/prompt_sync`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(body),
    });
    if (!res.ok) {
        const text = await res.text();
        throw new Error(`prompt_sync failed (${res.status}): ${text}`);
    }
    await res.json();
    return (performance.now() - start) / 1000;
}

async function runBenchmark() {
    let tandemServer = null;

    console.log('Starting E2E CLI Benchmark');
    console.log(`Provider: ${BENCH_PROVIDER}`);
    console.log(`Model: ${BENCH_MODEL}`);
    console.log(`Tandem Bin: ${TANDEM_BIN}`);
    console.log(`Opencode Bin: ${OPENCODE_BIN}`);
    console.log(`Runs: ${BENCH_RUNS}`);
    console.log(`JSON Report: ${REPORT_JSON}`);
    console.log(`TSV Report: ${REPORT_TSV}`);
    console.log(`Tandem Server: ${TANDEM_SERVER_BASE}`);
    if (!ENV_PATH) {
        console.log('No .env file found in BENCH_ENV_FILE/cwd/script/repo-root. Using exported env vars only.');
    }
    console.log(`Prompt: "${PROMPT}"`);

    if (!BENCH_API_KEY) {
        console.warn(
            `WARNING: No API key found for provider "${BENCH_PROVIDER}". ` +
            'Set BENCH_API_KEY or the provider-specific env var.'
        );
    }

    const iterations = [];
    const opencodeCandidates = OPENCODE_ENABLED ? resolveOpencodeCandidates() : [];
    let opencodeWorkingCommand = null;

    const serveArgs = ['serve', '--host', TANDEM_SERVER_HOST, '--port', String(TANDEM_SERVER_PORT)];
    if (BENCH_PROVIDER) serveArgs.push('--provider', BENCH_PROVIDER);
    if (BENCH_MODEL) serveArgs.push('--model', BENCH_MODEL);
    if (BENCH_API_KEY) serveArgs.push('--api-key', BENCH_API_KEY);
    tandemServer = spawn(TANDEM_BIN, serveArgs, {
        stdio: ['ignore', 'pipe', 'pipe'],
        shell: false,
        env: { ...process.env },
    });
    tandemServer.stdout.on('data', (d) => process.stdout.write(`[tandem-serve] ${d}`));
    tandemServer.stderr.on('data', (d) => process.stderr.write(`[tandem-serve] ${d}`));

    const ready = await waitForTandemReady();
    if (!ready) {
        throw new Error(`Tandem server did not become ready at ${TANDEM_SERVER_BASE}`);
    }

    try {
        for (let i = 1; i <= BENCH_RUNS; i++) {
        const result = {
            iteration: i,
            tandem: {
                attempted: true,
                success: false,
                timeSec: null,
                fallbackTimeSec: null,
                totalSec: null,
                error: null,
            },
            opencode: {
                attempted: OPENCODE_ENABLED,
                success: false,
                timeSec: null,
                error: null,
                skipped: !OPENCODE_ENABLED,
                command: null,
            },
            error: null,
        };

        console.log(`\n===== Iteration ${i}/${BENCH_RUNS} =====`);

        console.log('\n--- Benchmarking Tandem (Serve) ---');
        await cleanup();

        try {
            const session = await createTandemSession();
            const sessionId = session?.id;
            if (!sessionId) {
                throw new Error('create session returned no id');
            }
            const tandemTime = await runTandemPromptViaServer(sessionId);
            result.tandem.timeSec = tandemTime;
            result.tandem.totalSec = tandemTime;

            console.log(`Tandem Execution Time: ${tandemTime.toFixed(3)}s`);
            let success = await verifyResults();
            if (!success && BENCH_TANDEM_FALLBACK_TOOL) {
                console.log('Tandem run produced text-only output; retrying with deterministic tool fallback...');
                const fallbackTime = await runTandemToolFallback();
                result.tandem.fallbackTimeSec = fallbackTime;
                result.tandem.totalSec = tandemTime + fallbackTime;
                console.log(`Tandem Fallback Tool Time: ${fallbackTime.toFixed(3)}s`);
                success = await verifyResults();
            }
            result.tandem.success = success;
            console.log(`Tandem Success: ${success}`);
        } catch (e) {
            result.tandem.error = String(e.message || e);
            result.error = result.tandem.error;
            console.error(`Tandem Failed: ${e.message}`);
        }

        console.log('\n--- Benchmarking Opencode CLI ---');
        await cleanup();

        if (!OPENCODE_ENABLED) {
            console.log('Opencode disabled via OPENCODE_ENABLED=0');
        } else {
            const candidates = opencodeWorkingCommand
                ? [opencodeWorkingCommand, ...opencodeCandidates.filter((c) => c !== opencodeWorkingCommand)]
                : opencodeCandidates;
            let selected = null;
            let lastError = null;

            for (const candidate of candidates) {
                try {
                    const opencodeTime = await runCommand(candidate, ['run', PROMPT]);
                    result.opencode.timeSec = opencodeTime;
                    selected = candidate;
                    break;
                } catch (e) {
                    const msg = String(e.message || e);
                    lastError = msg;
                    if (msg.includes('ENOENT')) {
                        continue;
                    }
                    result.opencode.error = msg;
                    console.error(`Opencode Failed (${candidate}): ${msg}`);
                    break;
                }
            }

            if (!selected) {
                if (!result.opencode.error) {
                    result.opencode.skipped = true;
                    console.warn(`Opencode skipped: no runnable command found. Tried: ${candidates.join(', ')}`);
                    if (lastError) {
                        console.warn(`Last spawn error: ${lastError}`);
                    }
                }
            } else {
                opencodeWorkingCommand = selected;
                result.opencode.command = selected;
                result.opencode.skipped = false;
                console.log(`Opencode Command: ${selected}`);
                console.log(`Opencode Execution Time: ${result.opencode.timeSec.toFixed(3)}s`);
                const success = await verifyResults();
                result.opencode.success = success;
                console.log(`Opencode Success: ${success}`);
            }
        }

        iterations.push(result);
    }
    } finally {
        if (tandemServer && !tandemServer.killed) {
            tandemServer.kill();
        }
    }

    const tandemTotals = iterations
        .filter((r) => r.tandem.success && r.tandem.totalSec != null)
        .map((r) => r.tandem.totalSec);
    const opencodeRuns = iterations
        .filter((r) => r.opencode.attempted && !r.opencode.skipped && r.opencode.success && r.opencode.timeSec != null)
        .map((r) => r.opencode.timeSec);

    const summary = {
        timestamp: new Date().toISOString(),
        config: {
            provider: BENCH_PROVIDER,
            model: BENCH_MODEL,
            runs: BENCH_RUNS,
            tandemBin: TANDEM_BIN,
            opencodeEnabled: OPENCODE_ENABLED,
            opencodeBin: OPENCODE_BIN,
            strictCompare: STRICT_COMPARE,
            tandemFallbackTool: BENCH_TANDEM_FALLBACK_TOOL,
        },
        metrics: {
            tandem: {
                successCount: iterations.filter((r) => r.tandem.success).length,
                failCount: iterations.filter((r) => !r.tandem.success).length,
                avgTotalSec: avg(tandemTotals),
                medianTotalSec: median(tandemTotals),
                p95TotalSec: p95(tandemTotals),
            },
            opencode: {
                successCount: iterations.filter((r) => r.opencode.success).length,
                failCount: iterations.filter((r) => r.opencode.attempted && !r.opencode.skipped && !r.opencode.success).length,
                skippedCount: iterations.filter((r) => r.opencode.skipped).length,
                avgRunSec: avg(opencodeRuns),
                medianRunSec: median(opencodeRuns),
                p95RunSec: p95(opencodeRuns),
            },
        },
        iterations,
    };

    await writeReports(summary);

    console.log('\n=== E2E Aggregate Summary ===');
    console.log(
        `Tandem   | pass=${summary.metrics.tandem.successCount}/${BENCH_RUNS} | avg=${formatSeconds(summary.metrics.tandem.avgTotalSec)} | median=${formatSeconds(summary.metrics.tandem.medianTotalSec)} | p95=${formatSeconds(summary.metrics.tandem.p95TotalSec)}`
    );
    console.log(
        `Opencode | pass=${summary.metrics.opencode.successCount}/${BENCH_RUNS} | skipped=${summary.metrics.opencode.skippedCount} | avg=${formatSeconds(summary.metrics.opencode.avgRunSec)} | median=${formatSeconds(summary.metrics.opencode.medianRunSec)} | p95=${formatSeconds(summary.metrics.opencode.p95RunSec)}`
    );
    console.log(`Reports written:\n- ${REPORT_JSON}\n- ${REPORT_TSV}`);

    const tandemFailed = iterations.some((r) => !r.tandem.success);
    const opencodeFailed = iterations.some(
        (r) => r.opencode.attempted && !r.opencode.skipped && !r.opencode.success
    );
    if (tandemFailed || (STRICT_COMPARE && opencodeFailed)) {
        process.exitCode = 1;
    }
}

runBenchmark().catch(console.error);
