import test from "node:test";
import assert from "node:assert/strict";
import { mkdtemp, readFile } from "fs/promises";
import path from "path";
import os from "os";
import { spawn } from "child_process";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const packageRoot = path.resolve(__dirname, "..");
const cliPath = path.join(packageRoot, "index.js");

function runCli(args, cwd) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [cliPath, ...args], {
      cwd,
      stdio: "pipe",
      env: process.env,
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
});
