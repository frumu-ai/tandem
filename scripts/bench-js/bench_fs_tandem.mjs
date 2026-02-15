
import { spawn } from 'child_process';
import pLimit from 'p-limit';
import { resolve } from 'path';

const CONCURRENCY = 8;
const FILE_COUNT = 100;
const FILE_SIZE_KB = 10;
const PORT = 3002;
const ENGINE_BIN = '../../target/debug/tandem-engine.exe';
// Note: Relative path assumes running from tandem/scripts/bench-js/

const TEMP_DIR_REL = 'temp_bench_tandem';

// Helper to generate random content
const content = 'a'.repeat(FILE_SIZE_KB * 1024);

async function callTool(tool, args) {
    const res = await fetch(`http://127.0.0.1:${PORT}/tool/execute`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ tool, args })
    });

    if (!res.ok) {
        throw new Error(`HTTP ${res.status}: ${await res.text()}`);
    }
    const json = await res.json();
    // Check for sandbox denial or error in output
    if (json.output && (json.output.includes("denied") || json.output.startsWith("Unknown tool"))) {
        throw new Error(`Tool Error: ${json.output}`);
    }
    return json;
}

async function runBenchmark() {
    console.log(`Starting Tandem Engine FS Benchmark`);
    console.log(`Files: ${FILE_COUNT}, Size: ${FILE_SIZE_KB}KB, Concurrency: ${CONCURRENCY}`);

    // 1. START SERVER
    console.log(`Starting tandem-engine on port ${PORT}...`);
    const server = spawn(ENGINE_BIN, ['serve', '--port', PORT.toString()], {
        stdio: 'inherit',
        detached: false
    });

    // Wait for server ready
    let ready = false;
    for (let i = 0; i < 40; i++) {
        try {
            const res = await fetch(`http://127.0.0.1:${PORT}/global/health`);
            if (res.ok && (await res.json()).ready) {
                ready = true;
                break;
            }
        } catch { }
        await new Promise(r => setTimeout(r, 250));
    }

    if (!ready) {
        console.error('Server failed to start');
        server.kill();
        process.exit(1);
    }

    try {
        const limit = pLimit(CONCURRENCY);
        const fileNames = Array.from({ length: FILE_COUNT }, (_, i) => `file_${i}.txt`);

        // Ensure temp dir exists using bash tool (PowerShell mkdir creates parents by default)
        // Use force to ignore if exists
        await callTool('bash', { command: `mkdir "${TEMP_DIR_REL}" -Force` });

        // 2. WRITE BENCHMARK
        console.log('--- Benchmarking WRITE ---');
        const startWrite = performance.now();

        await Promise.all(fileNames.map(name => limit(async () => {
            // Use "write" tool
            await callTool('write', {
                path: `${TEMP_DIR_REL}/${name}`,
                content
            });
        })));

        const endWrite = performance.now();
        const writeDuration = (endWrite - startWrite) / 1000;
        console.log(`Write Time: ${writeDuration.toFixed(3)}s`);
        console.log(`Write Throughput: ${(FILE_COUNT / writeDuration).toFixed(0)} files/s`);

        // 3. READ BENCHMARK
        console.log('--- Benchmarking READ ---');
        const startRead = performance.now();

        await Promise.all(fileNames.map(name => limit(async () => {
            // Use "read" tool
            await callTool('read', {
                path: `${TEMP_DIR_REL}/${name}`
            });
        })));

        const endRead = performance.now();
        const readDuration = (endRead - startRead) / 1000;
        console.log(`Read Time: ${readDuration.toFixed(3)}s`);
        console.log(`Read Throughput: ${(FILE_COUNT / readDuration).toFixed(0)} files/s`);

        // 4. LIST (GLOB) BENCHMARK
        console.log('--- Benchmarking LIST (glob) ---');

        const startList = performance.now();

        // Use "glob" tool
        // Try relative path with forward slashes
        const globRes = await callTool('glob', {
            pattern: `${TEMP_DIR_REL}/*.txt`
        });

        // Parse the output (glob tool returns matching paths joined by newline)
        const files = globRes.output.split('\n').filter(Boolean);

        const endList = performance.now();
        const listDuration = (endList - startList) / 1000;

        console.log(`List Time: ${listDuration.toFixed(3)}s`);
        console.log(`Files Found: ${files.length}`);

        // 5. CLEANUP
        console.log('--- CLEANUP ---');
        // Verify files exist using local fs first
        await import('fs').then(fs => {
            if (fs.existsSync(TEMP_DIR_REL)) {
                const files = fs.readdirSync(TEMP_DIR_REL);
                console.log(`Node.js verified ${files.length} files in ${TEMP_DIR_REL}`);
            } else {
                console.log(`Node.js could not find ${TEMP_DIR_REL}`);
            }
        });

        // Use bash tool to remove temp dir
        await callTool('bash', { command: `rm -Ref "${TEMP_DIR_REL}"` });

        console.log('\n--- Results ---');
        console.log(`Tandem Write: ${writeDuration.toFixed(3)}s`);
        console.log(`Tandem Read:  ${readDuration.toFixed(3)}s`);
        console.log(`Tandem List:  ${listDuration.toFixed(3)}s`);

    } finally {
        console.log('Stopping server...');
        server.kill();
    }
}

runBenchmark().catch(err => {
    console.error(err);
    import('fs').then(fs => fs.writeFileSync('bench_error.log', err.toString() + '\\n' + err.stack));
});
