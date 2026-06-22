#!/usr/bin/env node

import http from "node:http";
import { mkdir, appendFile } from "node:fs/promises";
import path from "node:path";
import { randomUUID } from "node:crypto";
import { fileURLToPath } from "node:url";

const DEFAULT_HOST = "127.0.0.1";
const DEFAULT_PORT = 39741;

function parseArgs(argv) {
  const out = {
    host: DEFAULT_HOST,
    port: DEFAULT_PORT,
    outbox: path.resolve(".tmp/email-approval-demo/artifacts/outbox.jsonl"),
    drafts: path.resolve(".tmp/email-approval-demo/artifacts/drafts.jsonl"),
  };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    const [key, inline] = arg.split("=", 2);
    const value = inline ?? argv[i + 1];
    if (inline === undefined && arg.startsWith("--")) i += 1;
    if (key === "--host") out.host = value;
    if (key === "--port") out.port = Number(value);
    if (key === "--outbox") out.outbox = path.resolve(value);
    if (key === "--drafts") out.drafts = path.resolve(value);
  }
  return out;
}

function jsonResponse(res, status, body, sessionId = "email-demo-session") {
  const payload = JSON.stringify(body);
  res.writeHead(status, {
    "content-type": "application/json",
    "content-length": Buffer.byteLength(payload),
    "mcp-session-id": sessionId,
  });
  res.end(payload);
}

function rpcResult(id, result) {
  return { jsonrpc: "2.0", id: id ?? null, result };
}

function rpcError(id, code, message) {
  return { jsonrpc: "2.0", id: id ?? null, error: { code, message } };
}

function textToolResult(record) {
  return {
    content: [{ type: "text", text: JSON.stringify(record) }],
    structuredContent: record,
  };
}

async function appendJsonLine(file, value) {
  await mkdir(path.dirname(file), { recursive: true });
  await appendFile(file, `${JSON.stringify(value)}\n`, "utf8");
}

function readRequestBody(req) {
  return new Promise((resolve, reject) => {
    let raw = "";
    req.setEncoding("utf8");
    req.on("data", (chunk) => {
      raw += chunk;
      if (raw.length > 1024 * 1024) {
        reject(new Error("request too large"));
        req.destroy();
      }
    });
    req.on("end", () => resolve(raw));
    req.on("error", reject);
  });
}

const tools = [
  {
    name: "email.draft",
    description: "Create a demo email draft without sending it.",
    inputSchema: {
      type: "object",
      properties: {
        to: { type: "string" },
        subject: { type: "string" },
        body: { type: "string" },
        run_id: { type: "string" },
      },
      required: ["to", "subject", "body"],
      additionalProperties: true,
    },
  },
  {
    name: "email.send",
    description: "Append an approved demo email to the local outbox.",
    inputSchema: {
      type: "object",
      properties: {
        draft_id: { type: "string" },
        to: { type: "string" },
        subject: { type: "string" },
        body: { type: "string" },
        approved_by: { type: "string" },
        run_id: { type: "string" },
      },
      required: ["to", "subject", "body", "approved_by"],
      additionalProperties: true,
    },
  },
];

export function startEmailMcpServer(options) {
  const drafts = new Map();
  const server = http.createServer(async (req, res) => {
    if (req.method !== "POST" || req.url !== "/mcp") {
      jsonResponse(res, 404, { error: "not found" });
      return;
    }

    let rpc;
    try {
      rpc = JSON.parse(await readRequestBody(req));
    } catch (error) {
      jsonResponse(res, 400, rpcError(null, -32700, error.message));
      return;
    }

    if (rpc.method === "initialize") {
      jsonResponse(
        res,
        200,
        rpcResult(rpc.id, {
          protocolVersion: "2025-11-25",
          capabilities: { tools: {} },
          serverInfo: { name: "email-approval-demo", version: "0.1.0" },
        }),
      );
      return;
    }

    if (rpc.method === "tools/list") {
      jsonResponse(res, 200, rpcResult(rpc.id, { tools }));
      return;
    }

    if (rpc.method !== "tools/call") {
      jsonResponse(res, 200, rpcError(rpc.id, -32601, `unknown method ${rpc.method}`));
      return;
    }

    const name = rpc.params?.name;
    const args = rpc.params?.arguments ?? {};
    if (name === "email.draft") {
      const record = {
        type: "draft",
        draft_id: `draft-${randomUUID()}`,
        to: String(args.to ?? ""),
        subject: String(args.subject ?? ""),
        body: String(args.body ?? ""),
        run_id: args.run_id ? String(args.run_id) : null,
        created_at: new Date().toISOString(),
      };
      drafts.set(record.draft_id, record);
      await appendJsonLine(options.drafts, record);
      jsonResponse(res, 200, rpcResult(rpc.id, textToolResult(record)));
      return;
    }

    if (name === "email.send") {
      const draft = args.draft_id ? drafts.get(String(args.draft_id)) : null;
      const record = {
        type: "send",
        message_id: `msg-${randomUUID()}`,
        draft_id: args.draft_id ? String(args.draft_id) : null,
        to: String(args.to ?? draft?.to ?? ""),
        subject: String(args.subject ?? draft?.subject ?? ""),
        body: String(args.body ?? draft?.body ?? ""),
        approved_by: String(args.approved_by ?? ""),
        run_id: args.run_id ? String(args.run_id) : null,
        sent_at: new Date().toISOString(),
      };
      await appendJsonLine(options.outbox, record);
      jsonResponse(res, 200, rpcResult(rpc.id, textToolResult(record)));
      return;
    }

    jsonResponse(res, 200, rpcError(rpc.id, -32602, `unknown tool ${name}`));
  });

  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(options.port, options.host, () => {
      server.off("error", reject);
      resolve(server);
    });
  });
}

if (path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const options = parseArgs(process.argv.slice(2));
  const server = await startEmailMcpServer(options);
  const address = server.address();
  console.log(
    JSON.stringify({
      ok: true,
      server: "email-approval-demo-mcp",
      url: `http://${address.address}:${address.port}/mcp`,
      outbox: options.outbox,
      drafts: options.drafts,
    }),
  );

  const shutdown = () => server.close(() => process.exit(0));
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);
}
