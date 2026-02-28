import { renderMarkdown } from "../app/markdown.js";

const CHAT_UPLOAD_DIR = "control-panel";
const EXT_MIME = {
  md: "text/markdown",
  txt: "text/plain",
  csv: "text/csv",
  json: "application/json",
  pdf: "application/pdf",
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
};

function inferMime(name = "") {
  const ext = String(name).toLowerCase().split(".").pop() || "";
  return EXT_MIME[ext] || "application/octet-stream";
}

function joinRootAndRel(root, rel) {
  if (!root || !rel) return rel || "";
  const lhs = String(root).replace(/[\\/]+$/, "");
  const rhs = String(rel).replace(/^[\\/]+/, "");
  return `${lhs}/${rhs}`;
}

export async function renderChat(ctx) {
  const { state, byId, toast, escapeHtml, api, renderIcons } = ctx;
  const sessions = await loadSessions();
  if (!state.currentSessionId) state.currentSessionId = sessions[0]?.id || "";
  let sessionsOpen = false;

  byId("view").innerHTML = `
    <div id="chat-layout" class="chat-layout min-w-0 xl:h-[calc(100vh-12rem)]">
      <aside id="chat-sessions-panel" class="chat-sessions-panel">
        <div class="chat-sessions-header">
          <h3 class="chat-sessions-title"><i data-lucide="history"></i> Sessions</h3>
          <button id="new-session" class="tcp-btn h-8 px-2.5 text-xs"><i data-lucide="plus"></i> New</button>
        </div>
        <div id="session-list" class="chat-session-list"></div>
      </aside>
      <button id="chat-scrim" class="chat-scrim" aria-label="Close sessions"></button>
      <div class="chat-workspace min-h-0 min-w-0">
      <div class="chat-main-shell flex min-h-0 min-w-0 flex-col overflow-hidden">
        <div class="chat-main-header shrink-0">
          <button id="chat-toggle-sessions" type="button" class="chat-icon-btn h-8 w-8" title="Sessions"><i data-lucide="history"></i></button>
          <div class="chat-main-dot"></div>
          <h3 id="chat-title" class="tcp-title chat-main-title">Chat</h3>
          <span id="chat-tool-count" class="chat-main-tools hidden"></span>
        </div>
        <div id="messages" class="chat-messages mb-2 min-h-0 min-w-0 flex-1 space-y-2 overflow-auto p-3"></div>
        <div class="chat-composer shrink-0">
          <div id="chat-attach-row" class="chat-attach-row mb-2 hidden">
            <input id="chat-file-input" type="file" class="hidden" multiple />
            <span id="chat-attach-summary" class="tcp-subtle">0 files attached</span>
            <div id="chat-files" class="chat-files-line"></div>
          </div>
          <div id="chat-upload-progress" class="mb-2 grid gap-1.5"></div>
          <div class="chat-input-wrap">
            <button id="chat-file-pick-inner" type="button" class="chat-icon-btn chat-icon-btn-inner" title="Attach files"><i data-lucide="paperclip"></i></button>
            <textarea id="chat-input" rows="1" class="tcp-input chat-input-with-clip chat-input-modern resize-none border-slate-600/80 bg-slate-800/60" placeholder="Ask anything... (Enter to send, Shift+Enter newline)"></textarea>
            <button id="send-chat" class="chat-send-btn" title="Send"><i data-lucide="send"></i></button>
          </div>
        </div>
      </div>
      <aside class="chat-right-rail hidden min-h-0 flex-col gap-3 overflow-hidden xl:flex">
        <section class="min-h-0">
          <div class="mb-2 flex items-center justify-between">
            <p class="chat-rail-label">Tools</p>
            <span id="chat-rail-tools-count" class="chat-rail-count">0</span>
          </div>
          <div id="chat-tools-list" class="chat-tools-list"></div>
        </section>
        <section class="min-h-0 flex-1">
          <div class="mb-2 flex items-center justify-between">
            <p class="chat-rail-label">Tool Activity</p>
            <button id="chat-tools-clear" class="tcp-btn h-7 px-2 text-[11px]">Clear</button>
          </div>
          <div id="chat-tools-activity" class="chat-tools-activity"></div>
        </section>
      </aside>
      </div>
    </div>
  `;

  const layoutEl = byId("chat-layout");
  const sessionsPanelEl = byId("chat-sessions-panel");
  const scrimEl = byId("chat-scrim");
  const listEl = byId("session-list");
  const messagesEl = byId("messages");
  const inputEl = byId("chat-input");
  const sendEl = byId("send-chat");
  const fileInputEl = byId("chat-file-input");
  const filePickInnerEl = byId("chat-file-pick-inner");
  const attachRowEl = byId("chat-attach-row");
  const filesEl = byId("chat-files");
  const uploadProgressEl = byId("chat-upload-progress");
  const attachSummaryEl = byId("chat-attach-summary");
  const chatTitleEl = byId("chat-title");
  const chatToolCountEl = byId("chat-tool-count");
  const railToolsCountEl = byId("chat-rail-tools-count");
  const toolsListEl = byId("chat-tools-list");
  const toolsActivityEl = byId("chat-tools-activity");
  const uploadedFiles = Array.isArray(state.chatUploadedFiles) ? state.chatUploadedFiles : [];
  state.chatUploadedFiles = uploadedFiles;
  const uploadState = new Map();
  const toolActivity = [];
  const toolEventSeen = new Set();
  let availableTools = [];
  let sending = false;

  function setSessionsPanel(open) {
    sessionsOpen = !!open;
    layoutEl.classList.toggle("sessions-open", sessionsOpen);
    sessionsPanelEl.classList.toggle("open", sessionsOpen);
    scrimEl.classList.toggle("open", sessionsOpen);
  }

  function autosizeInput() {
    inputEl.style.height = "0px";
    inputEl.style.height = `${Math.min(inputEl.scrollHeight, 180)}px`;
  }

  async function loadSessions() {
    try {
      const list = await state.client.sessions.list({ pageSize: 50 });
      if (Array.isArray(list)) return list;
      if (Array.isArray(list?.sessions)) return list.sessions;
    } catch {
      // Fallback below handles older/newer response shapes via raw engine endpoint.
    }
    try {
      const raw = await api("/api/engine/session?page_size=50");
      if (Array.isArray(raw)) return raw;
      if (Array.isArray(raw?.sessions)) return raw.sessions;
      return [];
    } catch {
      return [];
    }
  }

  async function createSession() {
    const sid = await state.client.sessions.create({ title: `Chat ${new Date().toLocaleTimeString()}` });
    const rec = await state.client.sessions.get(sid).catch(() => ({ id: sid, title: sid }));
    sessions.unshift(rec);
    state.currentSessionId = sid;
    resetToolTracking();
    renderSessions();
    await renderMessages();
    return sid;
  }

  function formatBytes(bytes) {
    const n = Number(bytes || 0);
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
    return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  }

  function currentSessionTitle() {
    const session = sessions.find((s) => s.id === state.currentSessionId);
    const raw = String(session?.title || "").trim();
    if (raw) return raw;
    return state.currentSessionId ? `Session ${state.currentSessionId.slice(0, 8)}` : "Chat";
  }

  function setChatHeader() {
    chatTitleEl.textContent = currentSessionTitle();
    const count = availableTools.length;
    if (count > 0) {
      const label = `${count} tool${count === 1 ? "" : "s"}`;
      chatToolCountEl.textContent = label;
      chatToolCountEl.classList.remove("hidden");
      railToolsCountEl.textContent = String(count);
    } else {
      chatToolCountEl.classList.add("hidden");
      railToolsCountEl.textContent = "0";
    }
  }

  function toolStatusClass(status) {
    if (status === "completed") return "chat-tool-chip-ok";
    if (status === "failed") return "chat-tool-chip-failed";
    return "chat-tool-chip-running";
  }

  function renderToolRail() {
    toolsListEl.innerHTML =
      availableTools
        .slice(0, 32)
        .map(
          (name) =>
            `<button type="button" class="chat-tool-pill" data-tool-insert="${escapeHtml(name)}" title="Insert ${escapeHtml(name)}">${escapeHtml(name)}</button>`
        )
        .join("") || '<p class="chat-rail-empty">No tools loaded.</p>';
    toolsListEl.querySelectorAll("[data-tool-insert]").forEach((el) => {
      el.addEventListener("click", () => {
        const tool = String(el.dataset.toolInsert || "").trim();
        if (!tool) return;
        inputEl.value = inputEl.value.trim() ? `${inputEl.value} ${tool}` : tool;
        inputEl.focus();
      });
    });

    toolsActivityEl.innerHTML =
      toolActivity
        .slice(0, 24)
        .map((entry) => {
          const at = new Date(entry.at).toLocaleTimeString();
          return `<div class="chat-tool-chip ${toolStatusClass(entry.status)}" title="${escapeHtml(at)}">${escapeHtml(entry.tool)}: ${escapeHtml(entry.status)}</div>`;
        })
        .join("") || '<p class="chat-rail-empty">No tool events yet.</p>';
    setChatHeader();
  }

  function resetToolTracking() {
    toolActivity.splice(0, toolActivity.length);
    toolEventSeen.clear();
    renderToolRail();
  }

  function recordToolActivity(toolName, status, eventKey = "") {
    const tool = String(toolName || "").trim();
    if (!tool) return;
    if (eventKey) {
      if (toolEventSeen.has(eventKey)) return;
      toolEventSeen.add(eventKey);
      if (toolEventSeen.size > 1000) toolEventSeen.clear();
    }
    toolActivity.unshift({
      id: `${tool}:${status}:${Date.now()}:${Math.random().toString(36).slice(2, 8)}`,
      tool,
      status,
      at: Date.now(),
    });
    if (toolActivity.length > 80) toolActivity.length = 80;
    renderToolRail();
  }

  function extractToolsFromPayload(raw) {
    const list = Array.isArray(raw) ? raw : Array.isArray(raw?.tools) ? raw.tools : [];
    return list
      .map((item) => {
        if (typeof item === "string") return item;
        const rec = item || {};
        return String(rec.name || rec.id || rec.tool || "").trim();
      })
      .filter(Boolean);
  }

  async function refreshAvailableTools() {
    try {
      const direct = await state.client.listTools().catch(() => null);
      let ids = extractToolsFromPayload(direct || []);
      if (!ids.length) {
        const fallback = await api("/api/engine/tool").catch(() => []);
        ids = extractToolsFromPayload(fallback || []);
      }
      availableTools = [...new Set(ids)].sort((a, b) => a.localeCompare(b));
    } catch {
      availableTools = [];
    }
    renderToolRail();
  }

  function renderUploadProgress() {
    const rows = [...uploadState.entries()];
    if (!rows.length) {
      uploadProgressEl.innerHTML = "";
      return;
    }
    uploadProgressEl.innerHTML = rows
      .map(([id, item]) => {
        const pct = Math.max(0, Math.min(100, Number(item.progress || 0)));
        return `
          <div class="rounded-lg border border-slate-700/70 bg-slate-900/40 px-2 py-1.5">
            <div class="mb-1 flex items-center justify-between gap-2 text-xs">
              <span class="truncate text-slate-200">${escapeHtml(item.name)}</span>
              <span class="${item.error ? "text-rose-300" : "text-slate-400"}">${item.error ? escapeHtml(item.error) : `${pct}%`}</span>
            </div>
            <div class="h-1.5 overflow-hidden rounded-full bg-slate-800">
              <div class="h-full rounded-full bg-slate-400/80 transition-all duration-150" style="width:${pct}%"></div>
            </div>
          </div>
        `;
      })
      .join("");
  }

  function renderUploadedFiles() {
    if (!uploadedFiles.length) {
      filesEl.innerHTML = "";
      attachSummaryEl.textContent = "";
      attachRowEl.classList.add("hidden");
      return;
    }
    const attachedCount = uploadedFiles.length;
    attachSummaryEl.textContent = `${attachedCount} attached`;
    attachRowEl.classList.remove("hidden");
    filesEl.innerHTML = uploadedFiles
      .map(
        (f, idx) => `
          <div class="chat-file-pill min-w-0">
            <span class="chat-file-pill-name" title="${escapeHtml(f.path)}">${escapeHtml(f.path)}</span>
            <span class="chat-file-pill-size">${escapeHtml(formatBytes(f.size))}</span>
            <button class="chat-file-pill-btn chat-file-pill-btn-danger" type="button" data-file-remove="${idx}" title="Remove from list"><i data-lucide="x"></i></button>
          </div>
        `
      )
      .join("");
    filesEl.querySelectorAll("[data-file-remove]").forEach((el) => {
      el.addEventListener("click", () => {
        const i = Number(el.dataset.fileRemove);
        if (!Number.isFinite(i)) return;
        uploadedFiles.splice(i, 1);
        renderUploadedFiles();
      });
    });

    renderIcons(filesEl);
  }

  function uploadOne(file) {
    return new Promise((resolve, reject) => {
      const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
      uploadState.set(id, { name: file.name, progress: 0, error: "" });
      renderUploadProgress();

      const xhr = new XMLHttpRequest();
      xhr.open("POST", `/api/files/upload?dir=${encodeURIComponent(CHAT_UPLOAD_DIR)}`);
      xhr.withCredentials = true;
      xhr.responseType = "json";
      xhr.setRequestHeader("x-file-name", encodeURIComponent(file.name));

      xhr.upload.onprogress = (ev) => {
        if (!ev.lengthComputable) return;
        const row = uploadState.get(id);
        if (!row) return;
        row.progress = (ev.loaded / ev.total) * 100;
        renderUploadProgress();
      };

      xhr.onerror = () => {
        const row = uploadState.get(id);
        if (row) row.error = "Network error";
        renderUploadProgress();
        setTimeout(() => {
          uploadState.delete(id);
          renderUploadProgress();
        }, 1200);
        reject(new Error(`Upload failed: ${file.name}`));
      };

      xhr.onload = () => {
        const payload = xhr.response || {};
        if (xhr.status < 200 || xhr.status >= 300 || payload?.ok === false) {
          const message = payload?.error || `Upload failed (${xhr.status})`;
          const row = uploadState.get(id);
          if (row) row.error = String(message);
          renderUploadProgress();
          setTimeout(() => {
            uploadState.delete(id);
            renderUploadProgress();
          }, 1800);
          reject(new Error(String(message)));
          return;
        }
        uploadState.delete(id);
        renderUploadProgress();
        resolve(payload);
      };

      xhr.send(file);
    });
  }

  async function uploadFiles(fileList) {
    const files = [...(fileList || [])];
    if (!files.length) return;
    let success = 0;
    for (const file of files) {
      try {
        const rec = await uploadOne(file);
        uploadedFiles.unshift({
          name: rec.name || file.name,
          path: rec.path || file.name,
          size: Number(rec.size || file.size || 0),
          mime: file.type || inferMime(rec.name || file.name),
          url: rec.absPath || joinRootAndRel(rec.root, rec.path) || rec.path || file.name,
        });
        success += 1;
        renderUploadedFiles();
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    }
    if (success > 0) toast("ok", `Uploaded ${success} file${success === 1 ? "" : "s"}.`);
  }

  async function removeSession(sessionId) {
    await state.client.sessions.delete(sessionId);
    const idx = sessions.findIndex((s) => s.id === sessionId);
    if (idx >= 0) sessions.splice(idx, 1);
    if (state.currentSessionId === sessionId) {
      state.currentSessionId = sessions[0]?.id || "";
      if (!state.currentSessionId) await createSession();
    }
    resetToolTracking();
    renderSessions();
    await renderMessages();
  }

  function renderSessions() {
    listEl.innerHTML = sessions
      .map(
        (s) => `
          <div class="chat-session-row">
            <button class="chat-session-btn ${s.id === state.currentSessionId ? "active" : ""}" data-sid="${s.id}" title="${escapeHtml(s.id)}">
              <span class="block truncate">${escapeHtml(s.title || s.id.slice(0, 8))}</span>
            </button>
            <button class="chat-session-del" data-del-sid="${s.id}" title="Delete session">
              <i data-lucide="trash-2"></i>
            </button>
          </div>
        `
      )
      .join("");

    listEl.querySelectorAll("[data-sid]").forEach((btn) => {
      btn.addEventListener("click", async () => {
        state.currentSessionId = btn.dataset.sid;
        resetToolTracking();
        renderSessions();
        await renderMessages();
        setSessionsPanel(false);
      });
    });

    listEl.querySelectorAll("[data-del-sid]").forEach((btn) => {
      btn.addEventListener("click", async (e) => {
        e.stopPropagation();
        const sid = btn.dataset.delSid;
        if (!sid) return;
        if (!window.confirm("Delete this session?")) return;
        try {
          await removeSession(sid);
          toast("ok", "Session deleted.");
        } catch (err) {
          toast("err", err instanceof Error ? err.message : String(err));
        }
      });
    });

    renderIcons(listEl);
  }

  async function renderMessages() {
    setChatHeader();
    if (!state.currentSessionId) {
      messagesEl.innerHTML = '<p class="tcp-subtle">Create a session to begin.</p>';
      return;
    }

    const messages = await state.client.sessions.messages(state.currentSessionId).catch(() => []);
    messagesEl.innerHTML = messages
      .map((m) => {
        const roleRaw = String(m?.info?.role || "unknown");
        const role = escapeHtml(roleRaw);
        const textRaw = (m.parts || []).map((p) => p.text || "").join("\n");
        const isAssistantLike = roleRaw === "assistant" || roleRaw === "system";
        const content = isAssistantLike
          ? `<div class="tcp-markdown tcp-markdown-ai">${renderMarkdown(textRaw)}</div>`
          : `<pre class="max-w-full whitespace-pre-wrap break-all font-mono text-xs text-slate-200">${escapeHtml(textRaw)}</pre>`;
        const roleClass = isAssistantLike ? "assistant" : "user";
        return `<div class="chat-msg ${roleClass}"><div class="chat-msg-role">${role}</div>${content}</div>`;
      })
      .join("");

    messagesEl.scrollTop = messagesEl.scrollHeight;
  }

  byId("new-session").addEventListener("click", async () => {
    try {
      await createSession();
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });

  async function sendPrompt() {
    if (sending) return;
    const promptRaw = inputEl.value.trim();
    const attached = uploadedFiles.slice();
    const prompt = promptRaw || (attached.length ? "Please analyze the attached file(s)." : "");
    if (!prompt && attached.length === 0) return;
    inputEl.value = "";
    autosizeInput();
    sending = true;
    sendEl.disabled = true;

    try {
      if (!state.currentSessionId) await createSession();
      if (attached.length > 0) {
        toast("info", `Sending with ${attached.length} attached file${attached.length === 1 ? "" : "s"}.`);
      }
      const parts = attached.map((f) => ({
        type: "file",
        mime: f.mime || inferMime(f.name || f.path),
        filename: f.name || f.path || "attachment",
        url: f.url || f.path,
      }));
      parts.push({ type: "text", text: prompt });
      const runResp = await fetch(
        `/api/engine/session/${encodeURIComponent(state.currentSessionId)}/prompt_async?return=run`,
        {
          method: "POST",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({ parts }),
        }
      );
      let runId = "";
      if (runResp.status === 409) {
        const conflict = await runResp.json().catch(() => ({}));
        runId =
          conflict?.activeRun?.runID ||
          conflict?.activeRun?.runId ||
          conflict?.activeRun?.run_id ||
          "";
      } else if (runResp.ok) {
        const payload = await runResp.json().catch(() => ({}));
        runId = payload?.runID || payload?.runId || payload?.run_id || "";
      } else {
        const body = await runResp.text().catch(() => "");
        throw new Error(`prompt_async failed (${runResp.status}): ${body}`);
      }
      if (!runId) throw new Error("No run ID returned from engine.");
      if (attached.length > 0) {
        uploadedFiles.splice(0, uploadedFiles.length);
        renderUploadedFiles();
      }
      let responseText = "";
      let gotDelta = false;
      const placeholder = document.createElement("div");
      placeholder.className = "chat-msg assistant";
      placeholder.innerHTML = `
        <div class="chat-msg-role">assistant</div>
        <div class="tcp-thinking" aria-live="polite">
          <span>Thinking</span>
          <i></i><i></i><i></i>
        </div>
        <pre class="streaming-msg hidden whitespace-pre-wrap break-all font-mono text-xs text-slate-200"></pre>
      `;
      messagesEl.appendChild(placeholder);
      messagesEl.scrollTop = messagesEl.scrollHeight;
      const thinking = placeholder.querySelector(".tcp-thinking");
      const pre = placeholder.querySelector(".streaming-msg");

      const terminalSuccessEvents = new Set([
        "run.complete",
        "run.completed",
        "session.run.finished",
        "session.run.completed",
      ]);
      const terminalFailureEvents = new Set([
        "run.failed",
        "session.run.failed",
        "run.cancelled",
        "run.canceled",
        "session.run.cancelled",
        "session.run.canceled",
      ]);

      for await (const event of state.client.stream(state.currentSessionId, runId)) {
        const evRunId = String(event.runId || event.runID || event.run_id || event.properties?.runID || "").trim();
        if (evRunId && evRunId !== runId) continue;
        if (event.type === "session.response") {
          const delta = String(event.properties?.delta || "");
          if (!delta) continue;
          gotDelta = true;
          if (thinking) thinking.classList.add("hidden");
          pre.classList.remove("hidden");
          responseText += delta;
          pre.textContent = responseText;
          messagesEl.scrollTop = messagesEl.scrollHeight;
        }
        if (event.type === "tool.called" || event.type === "tool_call.started") {
          const tool = String(event.properties?.tool || "tool");
          recordToolActivity(tool, "started", `${event.type}:${evRunId || runId}:${tool}:start`);
        }
        if (
          event.type === "tool.result" ||
          event.type === "tool_call.completed" ||
          event.type === "tool_call.failed"
        ) {
          const tool = String(event.properties?.tool || "tool");
          const failed = event.type === "tool_call.failed";
          recordToolActivity(
            tool,
            failed ? "failed" : "completed",
            `${event.type}:${evRunId || runId}:${tool}:${failed ? "failed" : "completed"}`
          );
        }
        if (event.type === "message.part.updated") {
          const part = event.properties?.part || {};
          const partType = String(part.type || "").trim();
          const tool = String(part.tool || part.toolName || "").trim();
          if (tool && partType === "tool_invocation") {
            const partId = String(part.id || "").trim();
            recordToolActivity(tool, "started", `${event.type}:${partId || evRunId || runId}:${tool}:start`);
          }
          if (tool && partType === "tool_result") {
            const partId = String(part.id || "").trim();
            const pState = String(part.state || "").toLowerCase();
            const failed = pState === "failed" || pState === "error";
            recordToolActivity(
              tool,
              failed ? "failed" : "completed",
              `${event.type}:${partId || evRunId || runId}:${tool}:${failed ? "failed" : "completed"}`
            );
          }
        }
        if (terminalFailureEvents.has(event.type)) {
          throw new Error(String(event.properties?.error || "Run failed."));
        }
        if (terminalSuccessEvents.has(event.type)) {
          break;
        }
      }

      if (!gotDelta && thinking) {
        thinking.innerHTML = "<span>Finalizing response...</span>";
      }
      await renderMessages();
      // Some engine versions flush final assistant text right after stream close.
      await new Promise((resolve) => setTimeout(resolve, 180));
      await renderMessages();
      await new Promise((resolve) => setTimeout(resolve, 220));
      await renderMessages();
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
      await renderMessages();
    } finally {
      sending = false;
      sendEl.disabled = false;
    }
  }

  sendEl.addEventListener("click", () => {
    void sendPrompt();
  });
  byId("chat-toggle-sessions")?.addEventListener("click", () => {
    setSessionsPanel(!sessionsOpen);
  });
  scrimEl?.addEventListener("click", () => {
    setSessionsPanel(false);
  });
  byId("chat-tools-clear")?.addEventListener("click", () => {
    resetToolTracking();
  });
  filePickInnerEl.addEventListener("click", () => {
    fileInputEl.click();
  });
  fileInputEl.addEventListener("change", async () => {
    await uploadFiles(fileInputEl.files);
    fileInputEl.value = "";
  });
  inputEl.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      void sendPrompt();
    }
  });
  inputEl.addEventListener("input", () => {
    autosizeInput();
  });

  renderSessions();
  renderUploadedFiles();
  renderToolRail();
  void refreshAvailableTools();
  if (!state.currentSessionId && sessions.length === 0) {
    await createSession().catch(() => {});
  }
  setSessionsPanel(false);
  autosizeInput();
  await renderMessages();
}
