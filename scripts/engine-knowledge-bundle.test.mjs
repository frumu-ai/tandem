import test from "node:test";
import assert from "node:assert/strict";

import { normalizeDocContent, sha256Hex } from "./engine-knowledge-bundle.mjs";

test("normalizeDocContent canonicalizes CRLF and CR to LF", () => {
  assert.equal(normalizeDocContent("a\r\nb\r\n"), "a\nb\n");
  assert.equal(normalizeDocContent("a\rb\rc"), "a\nb\nc");
});

test("content hashes match after line-ending normalization", () => {
  const lf = normalizeDocContent("alpha\nbeta\n");
  const crlf = normalizeDocContent("alpha\r\nbeta\r\n");

  assert.equal(crlf, lf);
  assert.equal(sha256Hex(crlf), sha256Hex(lf));
});

test("normalized byte counts ignore raw CRLF width", () => {
  const lf = normalizeDocContent("hello\nworld\n");
  const crlf = normalizeDocContent("hello\r\nworld\r\n");

  assert.equal(Buffer.byteLength(lf, "utf8"), Buffer.byteLength(crlf, "utf8"));
  assert.equal(Buffer.byteLength(crlf, "utf8"), 12);
});
