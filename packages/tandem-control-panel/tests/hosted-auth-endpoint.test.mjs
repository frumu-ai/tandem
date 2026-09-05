import assert from "node:assert/strict";
import test from "node:test";
import { hostedAuthEndpoint } from "../lib/setup/hosted-auth-endpoint.js";

function config(base = "https://control.example.test") {
  return { managed: true, controlPlaneUrl: base,
    panelExchangeUrl: `${base}/exchange`, panelRefreshUrl: `${base}/refresh` };
}

test("hosted auth accepts the configured HTTPS origin and literal loopback adapters", () => {
  for (const base of ["https://control.example.test", "https://control.example.test/adapter", "http://127.0.0.1:1234", "http://[::1]:1234"]) {
    for (const operation of ["exchange", "refresh"]) {
      assert.equal(hostedAuthEndpoint(config(base), operation).href, `${base}/${operation}`);
    }
  }
});

test("hosted credentials cannot be redirected by an endpoint override", () => {
  for (const endpoint of ["https://other.example.test/refresh", "https://control.example.test.evil.test/refresh",
    "http://control.example.test/refresh", "https://user:password@control.example.test/refresh",
    "https://control.example.test/refresh?token=placeholder", "https://control.example.test/refresh#fragment",
    "file:///tmp/refresh", "/refresh"]) {
    assert.throws(() => hostedAuthEndpoint({ ...config(), panelRefreshUrl: endpoint }, "refresh"));
  }
  assert.throws(() => hostedAuthEndpoint({ ...config("https://control.example.test/adapter"),
    panelExchangeUrl: "https://control.example.test/elsewhere/exchange" }, "exchange"));
});

test("hosted auth rejects invalid control-plane configuration before reading credentials", () => {
  for (const base of ["http://control.example.test", "http://127.0.0.1.evil.test", "https://user:password@control.example.test",
    "https://control.example.test?redirect=elsewhere", "file:///tmp/control", "", "/relative"]) {
    assert.throws(() => hostedAuthEndpoint(config(base), "exchange"));
  }
  assert.throws(() => hostedAuthEndpoint({ ...config(), managed: false }, "exchange"));
  assert.throws(() => hostedAuthEndpoint(config(), "unsupported"));
});
