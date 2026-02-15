
console.log("Script starting...");

import { readFile, writeFile } from 'fs/promises';
import { resolve } from 'path';
import TurndownService from 'turndown';
import { JSDOM } from 'jsdom';
import pLimit from 'p-limit';

const CONCURRENCY = 8;
const TIMEOUT_MS = 15000;

async function fetchAndConvert(url) {
  const start = performance.now();
  let rssStart = process.memoryUsage().rss;

  try {
    const controller = new AbortController();
    const timeout = setTimeout(() => controller.abort(), TIMEOUT_MS);

    const res = await fetch(url, { signal: controller.signal });
    clearTimeout(timeout);
    
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    
    const html = await res.text();
    
    // Strip noise (script, style, noscript)
    const dom = new JSDOM(html);
    const doc = dom.window.document;
    doc.querySelectorAll('script, style, noscript').forEach(el => el.remove());
    
    // Convert to Markdown
    const turndownService = new TurndownService();
    const markdown = turndownService.turndown(doc.body.innerHTML);
    
    const elapsed = (performance.now() - start) / 1000;
    const rssEnd = process.memoryUsage().rss;
    const rssKb = Math.round((rssEnd - rssStart) / 1024); // Delta RSS (approx)
    // Actually we want total RSS usage of the process, but in JS it's shared.
    // The Rust bench measures PER PROCESS RSS.
    // Here we have one process. We can measure total RSS.
    const totalRssKb = Math.round(process.memoryUsage().rss / 1024);

    return {
      url,
      elapsed,
      rssKb: totalRssKb,
      status: 'ok'
    };
  } catch (err) {
    return {
      url,
      elapsed: (performance.now() - start) / 1000,
      rssKb: Math.round(process.memoryUsage().rss / 1024),
      status: 'error',
      error: err.message
    };
  }
}

async function main() {
  const urlsPath = process.argv[2];
  if (!urlsPath) {
    console.error('Usage: node bench.js <urls_file>');
    process.exit(1);
  }

  const urlsContent = await readFile(urlsPath, 'utf-8');
  const urls = urlsContent.split('\n').map(u => u.trim()).filter(u => u);

  console.log(`Starting benchmark with ${CONCURRENCY} concurrent workers...`);
  console.log(`Total URLs: ${urls.length}`);

  const limit = pLimit(CONCURRENCY);
  const tasks = urls.map(url => limit(() => fetchAndConvert(url)));
  
  const results = [];
  let completed = 0;
  
  // Progress reporter
  const interval = setInterval(() => {
    process.stdout.write(`\r${completed} / ${urls.length} completed`);
  }, 200);

  for (const task of tasks) {
    const res = await task;
    completed++;
    results.push(res);
  }
  
  clearInterval(interval);
  console.log('\nDone.');

  // Stats
  const elapsedTimes = results.map(r => r.elapsed).sort((a, b) => a - b);
  const rssValues = results.map(r => r.rssKb).sort((a, b) => a - b);
  
  const p50_elapsed = elapsedTimes[Math.floor(elapsedTimes.length * 0.5)];
  const p95_elapsed = elapsedTimes[Math.floor(elapsedTimes.length * 0.95)];
  
  const p50_rss = rssValues[Math.floor(rssValues.length * 0.5)];
  const p95_rss = rssValues[Math.floor(rssValues.length * 0.95)];

  console.log(`runs=${results.length}`);
  console.log(`p50_elapsed_s=${p50_elapsed.toFixed(3)}`);
  console.log(`p95_elapsed_s=${p95_elapsed.toFixed(3)}`);
  console.log(`p50_rss_kb=${p50_rss}`); // Note: This is single process RSS, which grows.
  // In Rust bench, we spawn a new process per URL (or pool of processes).
  // Comparing RSS of a long-running JS process vs short-lived Rust processes is tricky.
  // Rust processes are ~40MB each.
  // JS process will accumulate memory.
  // We should report the max RSS reached or the p50 of RSS samples?
  // Since JS is GC'd, RSS might be high.
  // Let's report what we observed.
  console.log(`p95_rss_kb=${p95_rss}`);
  
  // Write TSV
  const tsv = results.map(r => `${r.url}\t${r.elapsed}\t${r.rssKb}`).join('\n');
  await writeFile('results_js.tsv', tsv);
}

main().catch(console.error);
