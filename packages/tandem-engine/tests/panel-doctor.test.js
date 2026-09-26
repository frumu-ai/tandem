const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const { EventEmitter } = require("node:events");
const { createRequire } = require("node:module");

function fixture(exitCode = 0, stdout) {
  const filename = path.resolve(__dirname, "../bin/tandem.js");
  const localRequire = createRequire(filename);
  const calls = [];
  const output = [];
  const context = {
    module: { exports: {} }, __dirname: path.dirname(filename),
    process: { platform: "linux", env: { PATH: "/fixture/bin" }, cwd: () => "/fixture" },
    setTimeout, clearTimeout,
    console: { log: (...args) => output.push(args.join(" ")) },
    require(name) {
      if (name === "fs") return { ...fs, statSync: () => ({ isFile: () => true }) };
      if (name === "child_process") return { spawn(bin, args) {
        calls.push([bin, ...args]);
        const child = new EventEmitter();
        child.stdout = new EventEmitter();
        child.stderr = new EventEmitter();
        queueMicrotask(() => {
          if (bin === "tandem-setup") child.stdout.emit("data", Buffer.from(stdout ?? JSON.stringify({
            ok: exitCode === 0, engineHealth: { ready: exitCode === 0, healthy: exitCode === 0 },
            panelHost: "127.0.0.1", panelPort: 45678,
            panelPublicUrl: "https://panel.example.test", engineUrl: "http://127.0.0.1:45679",
          })));
          child.emit("close", bin === "tandem-setup" ? exitCode : 0);
        });
        return child;
      } };
      return localRequire(name);
    },
  };
  vm.runInNewContext(fs.readFileSync(filename, "utf8"), context, { filename });
  return { cli: context.module.exports, calls, output };
}

test("master panel commands preserve the add-on subcommand and flags", async () => {
  for (const command of ["doctor", "init", "run", "service"]) {
    const f = fixture();
    await f.cli.handlePanelCommand(command, { argv: [command, "--json"] });
    assert.deepEqual(f.calls, [["tandem-setup", command, "--json"]]);
  }
});

test("panel status preserves unhealthy doctor JSON and failure status", async () => {
  for (const exitCode of [0, 1]) {
    const f = fixture(exitCode);
    assert.equal(await f.cli.handlePanelCommand("status", { argv: ["status"] }), exitCode);
    assert.ok(f.output.some((line) => line.includes("127.0.0.1:45678")));
    assert.ok(f.output.some((line) => line.includes("127.0.0.1:45679")));
  }
});

test("panel open retains configured URL when doctor reports unhealthy", async () => {
  const f = fixture(1);
  await f.cli.handlePanelCommand("open", { argv: ["open"] });
  assert.deepEqual(f.calls.at(-1), ["xdg-open", "https://panel.example.test"]);
});

test("malformed failed doctor output is not treated as a report", async () => {
  for (const [code, output] of [[1, "not JSON"], [0, "null"], [0, "[]"], [0, '"text"'], [null, "{}"]]) {
    const f = fixture(code, output);
    assert.equal(await f.cli.handlePanelCommand("status", { argv: ["status"] }), 1);
    assert.ok(f.output.some((line) => line.includes("did not return")));
  }
});

test("panel open falls back when the doctor has no valid public URL", async () => {
  const f = fixture(1, JSON.stringify({ ok: false, engineHealth: null, engineUrl: "", panelPublicUrl: "", panelHost: "127.0.0.1", panelPort: 45678 }));
  await f.cli.handlePanelCommand("open", { argv: ["open"] });
  assert.deepEqual(f.calls.at(-1), ["xdg-open", "http://127.0.0.1:45678"]);
});

test("panel status rejects incomplete and wrongly typed doctor objects", async () => {
  const valid = {
    ok: true, panelHost: "127.0.0.1", panelPort: 45678,
    panelPublicUrl: "", engineUrl: "http://127.0.0.1:45679",
    engineHealth: { ready: true, healthy: true },
  };
  const invalid = [{}, { unrelated: true }];
  for (const field of Object.keys(valid)) {
    const missing = { ...valid };
    delete missing[field];
    invalid.push(missing);
  }
  for (const [field, values] of Object.entries({
    ok: ["true", null], panelHost: [null, "", 123], panelPort: ["45678", 0, -1, 65536, 1.5],
    panelPublicUrl: [null, {}], engineUrl: [null, 123, ""],
    engineHealth: [null, {}, [], { ready: "true", healthy: true }, { ready: true },
      { ready: false, healthy: true }, { ready: true, healthy: false }],
  })) {
    for (const value of values) invalid.push({ ...valid, [field]: value });
  }
  for (const report of invalid) {
    const f = fixture(0, JSON.stringify(report));
    assert.equal(await f.cli.handlePanelCommand("status", { argv: ["status"] }), 1, JSON.stringify(report));
    assert.ok(f.output.some((line) => line.includes("did not return")));
    assert.ok(f.output.every((line) => !line.includes("undefined")));
  }
});

test("panel status cannot turn a failed health report into success", async () => {
  const f = fixture(0, JSON.stringify({
    ok: false, panelHost: "127.0.0.1", panelPort: 45678,
    panelPublicUrl: "", engineUrl: "http://127.0.0.1:45679", engineHealth: null,
  }));
  assert.equal(await f.cli.handlePanelCommand("status", { argv: ["status"] }), 1);
  assert.ok(f.output.some((line) => line.includes("127.0.0.1:45678")));
});
