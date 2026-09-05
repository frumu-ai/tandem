import assert from "node:assert/strict";
import { copyFile, mkdir, mkdtemp, readFile, readdir, writeFile } from "node:fs/promises";
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import test from "node:test";
import { runDoctor } from "../lib/setup/doctor.js";

async function installedPanel() {
  const root = await mkdtemp(join(tmpdir(), "tandem-doctor-installed-"));
  const panel = join(root, "panel");
  await mkdir(join(panel, "lib/setup"), { recursive: true });
  await mkdir(join(panel, "dist"));
  // Isolated installed package: test actual resolution without changing the checkout.
  const engine = join(panel, "node_modules/@frumu/tandem/bin");
  await mkdir(engine, { recursive: true });
  await writeFile(join(engine, "tandem-engine.js"), "throw new Error('doctor must not start the engine');\n");
  await writeFile(join(panel, "package.json"), '{"type":"module"}');
  await mkdir(join(panel, "bin"));
  await mkdir(join(panel, "lib/setup/services"));
  await copyFile(new URL("../bin/cli.js", import.meta.url), join(panel, "bin/cli.js"));
  for (const name of ["doctor", "env", "paths", "common", "bootstrap", "services/systemd", "services/launchd", "services/common"]) {
    await copyFile(new URL(`../lib/setup/${name}.js`, import.meta.url), join(panel, `lib/setup/${name}.js`));
  }
  const { runDoctor } = await import(pathToFileURL(join(panel, "lib/setup/doctor.js")));
  const envFile = join(root, "panel.env");
  const source = "TANDEM_CONTROL_PANEL_ENGINE_TOKEN=synthetic-private-token\nTANDEM_ENGINE_URL=";
  const env = { HOME: root, XDG_CONFIG_HOME: root, XDG_DATA_HOME: root };
  const cli = () => new Promise((resolve, reject) => {
    const child = spawn(process.execPath, [join(panel, "bin/cli.js"), "doctor", "--json", "--env-file", envFile], {
      cwd: root, env: { ...env, PATH: process.env.PATH, ...(process.env.SystemRoot ? { SystemRoot: process.env.SystemRoot } : {}) },
      stdio: ["ignore", "pipe", "pipe"],
    });
    let output = "", error = "";
    child.stdout.on("data", (chunk) => { output += chunk; });
    child.stderr.on("data", (chunk) => { error += chunk; });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code, output, error }));
  });
  return { root, envFile, cli, run: async (url) => {
    await writeFile(envFile, source + url + "\n");
    const before = await readFile(envFile, "utf8");
    const result = await runDoctor({ cwd: root, envFile, env, allowAmbientStateEnv: false, allowCwdEnvMerge: false });
    assert.equal(await readFile(envFile, "utf8"), before, "doctor must not rewrite credentials/configuration");
    assert.equal(result.installed, true);
    assert.ok(!JSON.stringify(result).includes("synthetic-private-token"));
    assert.deepEqual((await readdir(root)).sort(), ["panel", "panel.env"], "doctor must not initialize state");
    return result;
  } };
}

test("installed packages cannot hide stopped, unready, invalid or recovered engine state", async (t) => {
  const panel = await installedPanel();
  let response = { ready: false, healthy: false };
  let status = 200;
  const seen = [];
  const server = createServer((request, reply) => {
    seen.push({ path: request.url, authorization: request.headers.authorization });
    reply.writeHead(status, { "content-type": "application/json" });
    reply.end(typeof response === "string" ? response : JSON.stringify(response));
  });
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  t.after(() => server.close());
  const url = `http://127.0.0.1:${server.address().port}`;
  for (const body of [{ ready: false, healthy: true }, { ready: true, healthy: false }, {}, null,
                       { ready: "true", healthy: true }, "not json synthetic-private-response"]) {
    response = body;
    const result = await panel.run(url);
    assert.equal(result.ok, false);
    assert.equal(result.running, true);
    assert.equal(result.runtimeReady, false);
    assert.equal(result.solutionReady, false);
    assert.ok(!JSON.stringify(result).includes("synthetic-private-response"));
  }
  status = 503;
  response = { ready: true, healthy: true };
  assert.equal((await panel.run(url)).checks[2].code, "engine_health_http_error");
  status = 200;
  response = { ready: true, healthy: true, private: "synthetic-private-response" };
  const recovered = await panel.run(url);
  assert.equal(recovered.ok, true);
  assert.equal(recovered.runtimeReady, true);
  assert.equal(recovered.solutionReady, false, "public liveness cannot establish authenticated solution readiness");
  assert.equal(recovered.authenticated, null);
  assert.equal(recovered.policyCurrent, null);
  assert.deepEqual(recovered.engineHealth, { ready: true, healthy: true });
  const recoveredCli = await panel.cli();
  assert.equal(recoveredCli.code, 0, recoveredCli.error);
  assert.equal(JSON.parse(recoveredCli.output).solutionReady, false);
  assert.ok(seen.every((request) => request.path === "/global/health" && request.authorization === undefined));
  await new Promise((resolve) => server.close(resolve));
  const stopped = await panel.run(url);
  assert.equal(stopped.ok, false, "previous green result must not survive loss of the engine");
  assert.equal(stopped.running, false);
  assert.equal(stopped.checks[2].code, "engine_unreachable");
  const stoppedCli = await panel.cli();
  assert.equal(stoppedCli.code, 1, stoppedCli.error);
  assert.equal(JSON.parse(stoppedCli.output).ok, false);
});

test("diagnostics reject credential-bearing URLs without sending or printing them", async () => {
  const panel = await installedPanel();
  const result = await panel.run("https://private-user:private-password@example.invalid?token=private-query");
  assert.equal(result.ok, false);
  assert.equal(result.checks[2].code, "engine_url_invalid");
  assert.equal(result.engineUrl, "https://example.invalid");
  for (const secret of ["private-user", "private-password", "private-query"]) {
    assert.ok(!JSON.stringify(result).includes(secret));
  }
});

test("doctor does not initialize a missing configuration or generate new credentials", async () => {
  const root = await mkdtemp(join(tmpdir(), "tandem-doctor-missing-"));
  const env = { HOME: root, XDG_CONFIG_HOME: root, XDG_DATA_HOME: root };
  await runDoctor({ cwd: root, envFile: join(root, "missing.env"), env,
    allowAmbientStateEnv: false, allowCwdEnvMerge: false });
  assert.deepEqual(await readdir(root), []);
});
