# Tandem Engine Awesome Examples

This companion guide to ENGINE_CLI.md focuses on advanced, real-world workflows that showcase tools, streaming, skills, MCP, multi-agent swarms, and planning. All examples assume the engine is running locally.

## Start the Engine

```bash
tandem-engine serve --hostname 127.0.0.1 --port 39731
```

## Tool Discovery (HTTP)

```bash
API="http://127.0.0.1:39731"
curl -s "$API/tool/ids"
curl -s "$API/tool"
```

## Agent and Skill Inventory (HTTP)

```bash
API="http://127.0.0.1:39731"
curl -s "$API/agent"
curl -s "$API/skills"
```

## Example Webpage Chat (HTML + SSE)

Create a local HTML file and serve it with any static server. This page sends messages to the engine and streams SSE events.

```bash
cat > chat.html << 'HTML'
<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Tandem Engine Chat</title>
    <style>
      body { font-family: system-ui, sans-serif; margin: 24px; }
      #log { white-space: pre-wrap; border: 1px solid #ddd; padding: 12px; height: 320px; overflow: auto; }
      #row { display: flex; gap: 8px; margin-top: 12px; }
      input { flex: 1; padding: 8px; }
      button { padding: 8px 12px; }
    </style>
  </head>
  <body>
    <h1>Tandem Engine Chat</h1>
    <div id="log"></div>
    <div id="row">
      <input id="prompt" placeholder="Say something..." />
      <button id="send">Send</button>
    </div>
    <script>
      const API = "http://127.0.0.1:39731";
      const log = document.getElementById("log");
      const promptInput = document.getElementById("prompt");
      const sendBtn = document.getElementById("send");

      function append(text) {
        log.textContent += text + "\n";
        log.scrollTop = log.scrollHeight;
      }

      async function createSession() {
        const res = await fetch(API + "/session", {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: "{}"
        });
        const json = await res.json();
        return json.id;
      }

      async function sendPrompt(sessionId, text) {
        const msg = { parts: [{ type: "text", text }] };
        await fetch(`${API}/session/${sessionId}/message`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(msg)
        });
        const runRes = await fetch(`${API}/session/${sessionId}/prompt_async?return=run`, {
          method: "POST",
          headers: { "content-type": "application/json" },
          body: JSON.stringify(msg)
        });
        const run = await runRes.json();
        return run.id;
      }

      async function start() {
        const sessionId = await createSession();
        append(`session: ${sessionId}`);
        sendBtn.onclick = async () => {
          const text = promptInput.value.trim();
          if (!text) return;
          promptInput.value = "";
          append(`you: ${text}`);
          const runId = await sendPrompt(sessionId, text);
          const stream = new EventSource(`${API}/event?sessionID=${sessionId}&runID=${runId}`);
          stream.onmessage = (evt) => {
            append(evt.data);
          };
          stream.onerror = () => {
            stream.close();
          };
        };
      }

      start();
    </script>
  </body>
</html>
HTML
python -m http.server 8080
```

Open http://127.0.0.1:8080/chat.html in a browser.

If you see a CORS error (for example when loading from file:// or localhost:8080), run a small local proxy and point the HTML to it:

```bash
node -e "import http from 'node:http';import {request} from 'node:http';const target='http://127.0.0.1:39731';http.createServer((req,res)=>{if(req.method==='OPTIONS'){res.writeHead(204,{'access-control-allow-origin':'*','access-control-allow-headers':'content-type,authorization,x-tandem-token','access-control-allow-methods':'GET,POST,PUT,PATCH,DELETE,OPTIONS'});return res.end();}const url=new URL(req.url,target);const proxyReq=request(url,{method:req.method,headers:req.headers},proxyRes=>{const headers={...proxyRes.headers,'access-control-allow-origin':'*','access-control-allow-headers':'content-type,authorization,x-tandem-token','access-control-allow-methods':'GET,POST,PUT,PATCH,DELETE,OPTIONS'};res.writeHead(proxyRes.statusCode||200,headers);proxyRes.pipe(res);});proxyReq.on('error',()=>{res.writeHead(502);res.end('bad gateway');});req.pipe(proxyReq);}).listen(8081);"
```

Then set `const API = "http://127.0.0.1:8081";` in the HTML file.

If you see a CORS error, use the same proxy snippet above and set `const API = "http://127.0.0.1:8081";`.

## Tool Gallery (CLI)

Each example uses the built-in tool runner to call tools directly.

### Workspace Navigation

```bash
tandem-engine tool --json '{"tool":"glob","args":{"pattern":"tandem/crates/**/*.rs"}}'
tandem-engine tool --json '{"tool":"grep","args":{"pattern":"EngineEvent","path":"tandem/crates"}}'
tandem-engine tool --json '{"tool":"codesearch","args":{"query":"prompt_async","path":"tandem"}}'
tandem-engine tool --json '{"tool":"read","args":{"path":"tandem/docs/ENGINE_CLI.md"}}'
```

### Write + Edit + Patch Validation

```bash
tandem-engine tool --json '{"tool":"write","args":{"path":"tmp/example.txt","content":"Hello from Tandem\n"}}'
tandem-engine tool --json '{"tool":"edit","args":{"path":"tmp/example.txt","old":"Hello","new":"Hola"}}'
tandem-engine tool --json "{\"tool\":\"apply_patch\",\"args\":{\"patchText\":\"*** Begin Patch\n*** Update File: tmp/example.txt\n@@\n-Hola from Tandem\n+Hello again from Tandem\n*** End Patch\n\"}}"
```

### Run a Shell Command

```bash
tandem-engine tool --json '{"tool":"bash","args":{"command":"Get-ChildItem tandem/docs | Select-Object -First 5"}}'
```

### Web Research

```bash
tandem-engine tool --json '{"tool":"webfetch","args":{"url":"https://github.com/frumu-ai/tandem"}}'
tandem-engine tool --json '{"tool":"webfetch_document","args":{"url":"https://github.com/frumu-ai/tandem","return":"both","mode":"auto"}}'
tandem-engine tool --json '{"tool":"websearch","args":{"query":"Tandem engine SSE events","limit":5}}'
```

### Memory and LSP

```bash
tandem-engine tool --json '{"tool":"memory_search","args":{"query":"engine loop","project_id":"tandem","tier":"project","limit":5}}'
tandem-engine tool --json '{"tool":"lsp","args":{"operation":"symbols","query":"EngineLoop"}}'
```

### Questions, Tasks, and Todos

```bash
tandem-engine tool --json '{"tool":"question","args":{"questions":[{"question":"Which provider should I use?","choices":["openrouter","openai","ollama"]}]}}'
tandem-engine tool --json '{"tool":"task","args":{"description":"Scan server routes","prompt":"Find the most important HTTP endpoints and summarize them."}}'
tandem-engine tool --json '{"tool":"todo_write","args":{"todos":[{"id":"demo-1","content":"Collect tool schemas","status":"pending"}]}}'
```

### Skills

```bash
tandem-engine tool --json '{"tool":"skill","args":{}}'
tandem-engine tool --json '{"tool":"skill","args":{"name":"RepoSummarizer"}}'
```

## Skills: Import and Use (HTTP)

```bash
API="http://127.0.0.1:39731"
cat > /tmp/SKILL.md << 'SKILL'
---
name: RepoSummarizer
description: Summarize a repository using tool-assisted scans
---
Summarize the repository structure and key modules in 8 bullets.
SKILL
curl -s -X POST "$API/skills/import" -H "content-type: application/json" -d '{"file_or_path":"/tmp/SKILL.md","location":"project","conflict_policy":"overwrite"}'
curl -s "$API/skills"
curl -s "$API/skills/RepoSummarizer"
```

## MCP: Streaming Tool Calls

Use the MCP debug tool to call a streaming MCP endpoint and see the raw response.

```bash
tandem-engine tool --json '{"tool":"mcp_debug","args":{"url":"https://mcp.exa.ai/mcp","tool":"web_search_exa","args":{"query":"Rust structured concurrency","numResults":3}}}'
```

Register and connect an MCP server, then list available MCP resources.

```bash
API="http://127.0.0.1:39731"
curl -s -X POST "$API/mcp" -H "content-type: application/json" -d '{"name":"local-mcp","transport":"stdio"}'
curl -s -X POST "$API/mcp/local-mcp/connect"
curl -s "$API/mcp/resources"
```

## Event Streams: Live SSE

This uses the async run flow and attaches to the engine SSE stream.

```bash
API="http://127.0.0.1:39731"
SID=$(curl -s -X POST "$API/session" -H "content-type: application/json" -d "{}" | jq -r ".id")
MSG='{"parts":[{"type":"text","text":"Stream a short poem about shipbuilding."}]}'
curl -s -X POST "$API/session/$SID/message" -H "content-type: application/json" -d "$MSG" > /dev/null
RUN=$(curl -s -X POST "$API/session/$SID/prompt_async?return=run" -H "content-type: application/json" -d "$MSG")
RUN_ID=$(echo "$RUN" | jq -r ".id")
curl -N "$API/event?sessionID=$SID&runID=$RUN_ID"
```

## Multi-Agent Swarm: Parallel Specialists

Create multiple role-specific tasks, then synthesize the results.

```bash
cat > tasks.json << 'JSON'
{
  "tasks": [
    { "id": "planner", "prompt": "Outline a 3-step plan to add a new HTTP route to tandem-server", "provider": "openrouter" },
    { "id": "coder", "prompt": "List the files you would touch to add a new route under crates/tandem-server", "provider": "openrouter" },
    { "id": "reviewer", "prompt": "Identify potential pitfalls when adding routes to the engine API", "provider": "openrouter" }
  ]
}
JSON
tandem-engine parallel --json @tasks.json --concurrency 3
tandem-engine run "Combine the planner/coder/reviewer results into a single actionable checklist."
```

## Planning Mode Prompts

```bash
tandem-engine run "Create a 7-step execution plan to add an SSE endpoint that streams tool lifecycle events."
tandem-engine run "Draft a migration plan for moving from single-session tooling to shared engine mode."
```

## Batch Tool Orchestration

```bash
tandem-engine tool --json '{"tool":"batch","args":{"tool_calls":[{"tool":"glob","args":{"pattern":"tandem/docs/*.md"}},{"tool":"read","args":{"path":"tandem/docs/ENGINE_CLI.md"}},{"tool":"grep","args":{"pattern":"token","path":"tandem/docs"}}]}}'
```
