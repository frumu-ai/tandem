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
  const f = fixture(1, "not JSON");
  await f.cli.handlePanelCommand("status", { argv: ["status"] });
  assert.ok(f.output.some((line) => line.includes("did not return")));
});
