
import { writeFile, readFile, rm, mkdir } from 'fs/promises';
import { resolve, join } from 'path';
import fg from 'fast-glob';
import pLimit from 'p-limit';

const CONCURRENCY = 8;
const FILE_COUNT = 100;
const FILE_SIZE_KB = 10;
const TEMP_DIR = './temp_bench_opencode';

// Helper to generate random content
const content = 'a'.repeat(FILE_SIZE_KB * 1024);

async function cleanup() {
  await rm(TEMP_DIR, { recursive: true, force: true });
}

async function runBenchmark() {
  console.log(`Starting Opencode (Node.js) FS Benchmark`);
  console.log(`Files: ${FILE_COUNT}, Size: ${FILE_SIZE_KB}KB, Concurrency: ${CONCURRENCY}`);

  // 1. SETUP
  await cleanup();
  await mkdir(TEMP_DIR);

  const limit = pLimit(CONCURRENCY);
  const fileNames = Array.from({ length: FILE_COUNT }, (_, i) => `file_${i}.txt`);

  // 2. WRITE BENCHMARK
  console.log('--- Benchmarking WRITE ---');
  const startWrite = performance.now();
  
  await Promise.all(fileNames.map(name => limit(async () => {
    await writeFile(join(TEMP_DIR, name), content);
  })));

  const endWrite = performance.now();
  const writeDuration = (endWrite - startWrite) / 1000;
  console.log(`Write Time: ${writeDuration.toFixed(3)}s`);
  console.log(`Write Throughput: ${(FILE_COUNT / writeDuration).toFixed(0)} files/s`);

  // 3. READ BENCHMARK
  console.log('--- Benchmarking READ ---');
  const startRead = performance.now();

  await Promise.all(fileNames.map(name => limit(async () => {
    await readFile(join(TEMP_DIR, name));
  })));

  const endRead = performance.now();
  const readDuration = (endRead - startRead) / 1000;
  console.log(`Read Time: ${readDuration.toFixed(3)}s`);
  console.log(`Read Throughput: ${(FILE_COUNT / readDuration).toFixed(0)} files/s`);

  // 4. LIST (GLOB) BENCHMARK
  console.log('--- Benchmarking LIST (glob) ---');
  const startList = performance.now();

  // Opencode `list` tool accepts glob patterns. 
  // We use fast-glob to simulate this.
  const files = await fg(`${TEMP_DIR}/*.txt`);
  
  const endList = performance.now();
  const listDuration = (endList - startList) / 1000;
  
  console.log(`List Time: ${listDuration.toFixed(3)}s`);
  console.log(`Files Found: ${files.length}`);

  // 5. CLEANUP
  await cleanup();
  
  // Return stats for comparison
  return {
    write_s: writeDuration,
    read_s: readDuration,
    list_s: listDuration
  };
}

runBenchmark().catch(console.error);
