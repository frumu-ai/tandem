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
tandem-engine tool --json '{"tool":"memory_store","args":{"content":"Retry build once after cache busting if lockfile changed","project_id":"tandem","tier":"project","source":"postmortem_note"}}'
tandem-engine tool --json '{"tool":"memory_list","args":{"project_id":"tandem","tier":"all","limit":20}}'
# Global memory (cross-project) is opt-in:
tandem-engine tool --json '{"tool":"memory_store","args":{"content":"Prefer rg over grep for repository search speed","tier":"global","allow_global":true,"source":"coding_preference"}}'
tandem-engine tool --json '{"tool":"memory_search","args":{"query":"repository search speed","tier":"global","allow_global":true,"limit":5}}'
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

## Live Tool Approval Walkthrough

Trigger a tool that requires approval, then approve it via the HTTP endpoint while watching SSE.

```bash
API="http://127.0.0.1:39731"
SID=$(curl -s -X POST "$API/session" -H "content-type: application/json" -d "{}" | jq -r ".id")
MSG='{"parts":[{"type":"text","text":"/tool bash {\"command\":\"Get-Date\"}"}]}'
curl -s -X POST "$API/session/$SID/message" -H "content-type: application/json" -d "$MSG" > /dev/null
RUN=$(curl -s -X POST "$API/session/$SID/prompt_async?return=run" -H "content-type: application/json" -d "$MSG")
RUN_ID=$(echo "$RUN" | jq -r ".id")
curl -N "$API/event?sessionID=$SID&runID=$RUN_ID"
```

In a second terminal, grab the permission request ID from the SSE output and approve it:

```bash
REQUEST_ID="paste-request-id-from-permission.asked"
curl -s -X POST "$API/sessions/$SID/tools/$REQUEST_ID/approve"
```

## Session Replay + Timeline

Record the SSE stream, then extract a compact timeline of event types.

```bash
API="http://127.0.0.1:39731"
SID=$(curl -s -X POST "$API/session" -H "content-type: application/json" -d "{}" | jq -r ".id")
MSG='{"parts":[{"type":"text","text":"Summarize the last 3 Git commits in the repo."}]}'
curl -s -X POST "$API/session/$SID/message" -H "content-type: application/json" -d "$MSG" > /dev/null
RUN=$(curl -s -X POST "$API/session/$SID/prompt_async?return=run" -H "content-type: application/json" -d "$MSG")
RUN_ID=$(echo "$RUN" | jq -r ".id")
curl -N "$API/event?sessionID=$SID&runID=$RUN_ID" | tee sse.log
```

Extract a timeline (event type + runID):

```bash
cat sse.log \
  | rg "^data:" \
  | sed "s/^data: //g" \
  | jq -r '"\(.type)\t\(.properties.runID // .properties.runId // "-")"' \
  | uniq
```

## Server Playbook: Sessions + Run Control (HTTP)

Create a session, list sessions, start a run, inspect it, then cancel.

```bash
API="http://127.0.0.1:39731"
SID=$(curl -s -X POST "$API/session" -H "content-type: application/json" -d "{}" | jq -r ".id")
curl -s "$API/session"
MSG='{"parts":[{"type":"text","text":"Write a haiku about latency."}]}'
curl -s -X POST "$API/session/$SID/message" -H "content-type: application/json" -d "$MSG" > /dev/null
RUN=$(curl -s -X POST "$API/session/$SID/prompt_async?return=run" -H "content-type: application/json" -d "$MSG")
RUN_ID=$(echo "$RUN" | jq -r ".id")
curl -s "$API/session/$SID/run"
curl -s -X POST "$API/session/$SID/run/$RUN_ID/cancel"
```

## Server Playbook: Reattach After Disconnect (HTTP + SSE)

Recover an in-flight run by reattaching with the active run ID.

```bash
API="http://127.0.0.1:39731"
SID=$(curl -s -X POST "$API/session" -H "content-type: application/json" -d "{}" | jq -r ".id")
MSG='{"parts":[{"type":"text","text":"Explain three uses of SSE in apps."}]}'
curl -s -X POST "$API/session/$SID/message" -H "content-type: application/json" -d "$MSG" > /dev/null
RUN=$(curl -s -X POST "$API/session/$SID/prompt_async?return=run" -H "content-type: application/json" -d "$MSG")
RUN_ID=$(echo "$RUN" | jq -r ".id")
curl -N "$API/event?sessionID=$SID&runID=$RUN_ID"
ACTIVE=$(curl -s "$API/session/$SID/run" | jq -r ".runID // .runId // .id")
curl -N "$API/event?sessionID=$SID&runID=$ACTIVE"
```

## Server Playbook: Permissions + Questions (HTTP)

Poll for pending approvals/questions and reply over HTTP.

```bash
API="http://127.0.0.1:39731"
curl -s "$API/permission"
curl -s -X POST "$API/permission/<id>/reply" -H "content-type: application/json" -d '{"reply":"allow"}'
curl -s "$API/question"
curl -s -X POST "$API/question/<id>/reply" -H "content-type: application/json" -d '{"reply":"continue"}'
curl -s -X POST "$API/question/<id>/reject" -H "content-type: application/json" -d '{"reply":"stop"}'
```

## Server Playbook: Health + Phase (HTTP)

Use health to drive readiness checks and dashboards.

```bash
API="http://127.0.0.1:39731"
curl -s "$API/global/health" | jq
```

## Server Playbook: Mission Runtime (HTTP)

Create a mission, inspect it, then apply reducer events (review/test gates) through the engine API.

```bash
API="http://127.0.0.1:39731"

# Create a mission with one work item
MISSION=$(curl -s -X POST "$API/mission" -H "content-type: application/json" -d '{
  "title":"Ship routine policy gates",
  "goal":"Implement and verify connector side-effect policy for routines",
  "work_items":[
    {"title":"Add scheduler/run_now policy checks","detail":"Block/approval/allow paths"},
    {"title":"Add tests","detail":"HTTP + unit tests for policy outcomes"}
  ]
}')
echo "$MISSION" | jq

MISSION_ID=$(echo "$MISSION" | jq -r '.mission.mission_id')
WORK_ITEM_ID=$(echo "$MISSION" | jq -r '.mission.work_items[0].work_item_id')
echo "mission=$MISSION_ID work_item=$WORK_ITEM_ID"

# List + fetch
curl -s "$API/mission" | jq
curl -s "$API/mission/$MISSION_ID" | jq

# Simulate work item run completion -> reviewer gate
curl -s -X POST "$API/mission/$MISSION_ID/event" -H "content-type: application/json" -d "{
  \"event\": {
    \"type\": \"run_finished\",
    \"mission_id\": \"$MISSION_ID\",
    \"work_item_id\": \"$WORK_ITEM_ID\",
    \"run_id\": \"run-demo-1\",
    \"status\": \"completed\"
  }
}" | jq

# Approve reviewer -> tester gate
curl -s -X POST "$API/mission/$MISSION_ID/event" -H "content-type: application/json" -d "{
  \"event\": {
    \"type\": \"approval_granted\",
    \"mission_id\": \"$MISSION_ID\",
    \"work_item_id\": \"$WORK_ITEM_ID\",
    \"approval_id\": \"review-1\"
  }
}" | jq

# Approve tester -> work item done (mission may complete when all required items are done)
curl -s -X POST "$API/mission/$MISSION_ID/event" -H "content-type: application/json" -d "{
  \"event\": {
    \"type\": \"approval_granted\",
    \"mission_id\": \"$MISSION_ID\",
    \"work_item_id\": \"$WORK_ITEM_ID\",
    \"approval_id\": \"test-1\"
  }
}" | jq

curl -s "$API/mission/$MISSION_ID" | jq
```

## Server Playbook: Mission Events via SSE (HTTP)

Watch mission lifecycle events while creating and updating a mission.

```bash
API="http://127.0.0.1:39731"

# Terminal 1: watch only mission events
curl -N "$API/event" | jq -r 'select(.event_type|startswith("mission."))'
```

In a second terminal:

```bash
API="http://127.0.0.1:39731"
MISSION=$(curl -s -X POST "$API/mission" -H "content-type: application/json" -d '{
  "title":"SSE mission demo",
  "goal":"Observe mission.created and mission.updated events",
  "work_items":[{"title":"Demo item"}]
}')
MISSION_ID=$(echo "$MISSION" | jq -r '.mission.mission_id')
WORK_ITEM_ID=$(echo "$MISSION" | jq -r '.mission.work_items[0].work_item_id')

curl -s -X POST "$API/mission/$MISSION_ID/event" -H "content-type: application/json" -d "{
  \"event\": {
    \"type\": \"run_finished\",
    \"mission_id\": \"$MISSION_ID\",
    \"work_item_id\": \"$WORK_ITEM_ID\",
    \"run_id\": \"run-sse-1\",
    \"status\": \"completed\"
  }
}" | jq
```

## Server Playbook: Routine Policy Gates (HTTP)

This demonstrates the tiered outcomes for connector-backed routines:

- `queued`: external allowed + no approval required
- `pending_approval`: external allowed + approval required
- `ROUTINE_POLICY_BLOCKED`: external side effects disabled by policy

```bash
API="http://127.0.0.1:39731"

# 1) Blocked by policy (default-safe)
curl -s -X POST "$API/routines" -H "content-type: application/json" -d '{
  "routine_id":"email-blocked",
  "name":"Email blocked",
  "schedule":{"interval_seconds":{"seconds":300}},
  "entrypoint":"connector.email.reply",
  "requires_approval":true,
  "external_integrations_allowed":false
}' | jq
curl -s -X POST "$API/routines/email-blocked/run_now" -H "content-type: application/json" -d '{}' | jq

# 2) Approval required
curl -s -X POST "$API/routines" -H "content-type: application/json" -d '{
  "routine_id":"email-approval",
  "name":"Email approval",
  "schedule":{"interval_seconds":{"seconds":300}},
  "entrypoint":"connector.email.reply",
  "requires_approval":true,
  "external_integrations_allowed":true
}' | jq
curl -s -X POST "$API/routines/email-approval/run_now" -H "content-type: application/json" -d '{}' | jq

# 3) Allowed and queued
curl -s -X POST "$API/routines" -H "content-type: application/json" -d '{
  "routine_id":"email-queued",
  "name":"Email queued",
  "schedule":{"interval_seconds":{"seconds":300}},
  "entrypoint":"connector.email.reply",
  "requires_approval":false,
  "external_integrations_allowed":true
}' | jq
curl -s -X POST "$API/routines/email-queued/run_now" -H "content-type: application/json" -d '{}' | jq

curl -s "$API/routines/email-blocked/history?limit=5" | jq
curl -s "$API/routines/email-approval/history?limit=5" | jq
curl -s "$API/routines/email-queued/history?limit=5" | jq
```

## Server Playbook: Routine Events via SSE (HTTP)

Subscribe to routine lifecycle events and trigger transitions.

```bash
API="http://127.0.0.1:39731"

# Terminal 1: watch routine lifecycle stream
curl -N "$API/routines/events"
```

In a second terminal:

```bash
API="http://127.0.0.1:39731"
curl -s -X POST "$API/routines" -H "content-type: application/json" -d '{
  "routine_id":"sse-routine-demo",
  "name":"SSE routine demo",
  "schedule":{"interval_seconds":{"seconds":120}},
  "entrypoint":"mission.default",
  "creator_type":"user",
  "creator_id":"demo"
}' | jq

curl -s -X POST "$API/routines/sse-routine-demo/run_now" -H "content-type: application/json" -d '{"reason":"sse smoke"}' | jq
curl -s -X PATCH "$API/routines/sse-routine-demo" -H "content-type: application/json" -d '{"status":"paused"}' | jq
curl -s -X DELETE "$API/routines/sse-routine-demo" | jq
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
