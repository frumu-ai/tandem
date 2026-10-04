import test from "node:test";
import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, readdir, writeFile } from "fs/promises";
import { createServer } from "node:http";
import path from "path";
import os from "os";
import { spawn } from "child_process";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(__dirname, "..");
const cliPath = path.join(packageRoot, "index.js");

function runCli(args, cwd, executable = cliPath, env = process.env) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [executable, ...args], {
      cwd,
      stdio: "pipe",
      env,
    });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk.toString("utf8");
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk.toString("utf8");
    });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code: code || 0, stdout, stderr }));
  });
}

test("scaffold creates a standalone editable app payload", async () => {
  const tmpRoot = await mkdtemp(path.join(os.tmpdir(), "create-tandem-panel-"));
  const result = await runCli(["my-panel"], tmpRoot);
  assert.equal(result.code, 0, result.stderr || result.stdout);

  const generatedRoot = path.join(tmpRoot, "my-panel");
  const packageJson = JSON.parse(await readFile(path.join(generatedRoot, "package.json"), "utf8"));
  const themesSource = await readFile(path.join(generatedRoot, "src/app/themes.js"), "utf8");
  const viteSource = await readFile(path.join(generatedRoot, "vite.config.ts"), "utf8");
  const devRunner = await readFile(path.join(generatedRoot, "scripts/dev.js"), "utf8");
  const startRunner = await readFile(path.join(generatedRoot, "bin/setup.js"), "utf8");

  assert.equal(packageJson.name, "my-panel");
  assert.match(result.stdout, /npm run dev/);
  assert.doesNotMatch(themesSource, /tandem-theme-contract/);
  assert.doesNotMatch(viteSource, /tandem-client-ts/);
  assert.match(viteSource, /proxy/);
  assert.match(devRunner, /TANDEM_CONTROL_PANEL_DISABLE_STATIC/);
  assert.match(startRunner, /const REPO_ROOT = resolve\(__dirname, "\.\."\);/);
  assert.match(startRunner, /isPublicEngineAutomationWebhookPath/);
  assert.match(startRunner, /proxyPublicEngineAutomationWebhook/);
  assert.match(startRunner, /\["POST", "OPTIONS"\]/);
  assert.match(startRunner, /headers\.set\("x-forwarded-prefix", "\/api\/engine"\)/);
});

test("generated doctor is read-only and reports stopped, unready and ready engines", async (t) => {
  const root = await mkdtemp(path.join(os.tmpdir(), "generated-panel-doctor-"));
  const generated = await runCli(["my-panel"], root);
  assert.equal(generated.code, 0, generated.stderr);
  const panel = path.join(root, "my-panel");
  await mkdir(path.join(panel, "dist"));
  const engine = path.join(panel, "node_modules/@frumu/tandem/bin");
  await mkdir(engine, { recursive: true });
  await writeFile(path.join(engine, "tandem-engine.js"), "throw new Error('doctor must not start engine');\n");
  let ready = false;
  const server = createServer((request, response) => {
    assert.equal(request.url, "/global/health");
    assert.equal(request.headers.authorization, undefined);
    response.writeHead(200, { "content-type": "application/json" });
    response.end(JSON.stringify({ ready, healthy: ready }));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => server.close());
  const envFile = path.join(root, "panel.env");
  const contents = `TANDEM_ENGINE_URL=http://127.0.0.1:${server.address().port}\nCUSTOM=preserve\n`;
  await writeFile(envFile, contents);
  const before = (await readdir(root)).sort();
  const env = { ...process.env, HOME: root, XDG_CONFIG_HOME: root, XDG_DATA_HOME: root };
  const doctor = () => runCli(["doctor", "--json", "--env-file", envFile], root,
    path.join(panel, "bin/cli.js"), env);
  for (const expected of [false, true]) {
    ready = expected;
    const result = await doctor();
    assert.equal(result.code, expected ? 0 : 1, result.stderr);
    const report = JSON.parse(result.stdout);
    assert.equal(report.installed, true);
    assert.equal(report.runtimeReady, expected);
    assert.equal(report.solutionReady, false);
    assert.equal(await readFile(envFile, "utf8"), contents);
    assert.deepEqual((await readdir(root)).sort(), before);
  }
  await new Promise((resolve) => server.close(resolve));
  const stopped = await doctor();
  assert.equal(stopped.code, 1, stopped.stderr);
  assert.equal(JSON.parse(stopped.stdout).running, false);
  assert.equal(await readFile(envFile, "utf8"), contents);
});
