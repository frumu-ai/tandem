
import { readFile, writeFile } from 'fs/promises';
import { spawn } from 'child_process';
import pLimit from 'p-limit';

const CONCURRENCY = 8;
const TIMEOUT_MS = 60000;
const PORT = 3001; // Use a different port to avoid conflicts
const ENGINE_BIN = '../../target/debug/tandem-engine.exe';

async function fetchTool(url) {
  const start = performance.now();
  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), TIMEOUT_MS);

    const res = await fetch(`http://127.0.0.1:${PORT}/tool/execute`, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        tool: 'webfetch_document',
        args: { url }
      }),
      signal: controller.signal
    });
    clearTimeout(timeout);

    if (!res.ok) {
        const text = await res.text();
        throw new Error(`HTTP ${res.status}: ${text}`);
    }
    
    const json = await res.json();
    const elapsed = (performance.now() - start) / 1000;

    if (json.output && json.output.length < 100) {
       // Check if it's an error message disguised as output
       if (json.output.includes("error") || json.output.includes("Error")) {
           return {
               url,
               elapsed,
               status: 'error',
               error: json.output
           };
       }
    }
    
    return {
      url,
      elapsed,
      status: 'ok',
      output_preview: json.output ? json.output.substring(0, 50) : "no output"
    };
  } catch (err) {
    return {
      url,
      elapsed: (performance.now() - start) / 1000,
      status: 'error',
      error: err.message
    };
  }
}

async function main() {
  const urlsPath = process.argv[2];
  if (!urlsPath) {
    console.error('Usage: node bench_server.mjs <urls_file>');
    process.exit(1);
  }

  const urlsContent = await readFile(urlsPath, 'utf-8');
  const urls = urlsContent.split('\n').map(u => u.trim()).filter(u => u);

  console.log(`Starting tandem-engine serve on port ${PORT}...`);
  const server = spawn(ENGINE_BIN, ['serve', '--port', PORT.toString()], {
    stdio: 'inherit', // Useful for debugging if server fails
    detached: false
  });

  // Wait for server to be ready
  let ready = false;
  for (let i = 0; i < 20; i++) {
    try {
      const res = await fetch(`http://127.0.0.1:${PORT}/global/health`);
      if (res.ok) {
        const json = await res.json();
        if (json.ready) {
            ready = true;
            break;
        }
      }
    } catch (e) {
      // ignore
    }
    await new Promise(r => setTimeout(r, 500));
  }

  if (!ready) {
    console.error('Server failed to start');
    server.kill();
    process.exit(1);
  }

  console.log('Server ready. Starting benchmark...');
  console.log(`Total URLs: ${urls.length}`);

  const limit = pLimit(CONCURRENCY);
  const tasks = urls.map(url => limit(() => fetchTool(url)));
  
  const results = [];
  let completed = 0;
  
  const interval = setInterval(() => {
    process.stdout.write(`\r${completed} / ${urls.length} completed`);
  }, 200);

  for (const task of tasks) {
    const res = await task;
    completed++;
    results.push(res);
  }
  
  clearInterval(interval);
  console.log('\nDone. Stopping server...');
  server.kill();

  // Stats
  const elapsedTimes = results.map(r => r.elapsed).sort((a, b) => a - b);
  const p50 = elapsedTimes[Math.floor(elapsedTimes.length * 0.5)];
  const p95 = elapsedTimes[Math.floor(elapsedTimes.length * 0.95)];

  console.log(`runs=${results.length}`);

  const successful = results.filter(r => r.status === 'ok');
  console.log(`successful=${successful.length}`);
  if (successful.length > 0) {
    console.log(`First output preview: ${successful[0].output_preview}`);
  } else {
    const errors = results.filter(r => r.status === 'error');
    if (errors.length > 0) {
        console.log(`First error: ${errors[0].error}`);
    }
  }

  console.log(`p50_elapsed_s=${p50.toFixed(3)}`);
  console.log(`p95_elapsed_s=${p95.toFixed(3)}`);
  
  // Write TSV
  const tsv = results.map(r => `${r.url}\t${r.elapsed}\t${r.status}`).join('\n');
  await writeFile('results_server.tsv', tsv);
}

main().catch(console.error);
