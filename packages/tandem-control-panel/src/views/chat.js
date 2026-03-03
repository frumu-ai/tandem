import { renderMarkdown } from "../app/markdown.js";
import { confirmActionModal } from "../app/dom.js";

const CHAT_UPLOAD_DIR = "control-panel";
const CHAT_AUTO_APPROVE_KEY = "tandem_control_panel_chat_auto_approve_tools";
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

function loadAutoApprovePreference() {
  try {
    return localStorage.getItem(CHAT_AUTO_APPROVE_KEY) === "1";
  } catch {
    return false;
  }
}

function saveAutoApprovePreference(enabled) {
  try {
    localStorage.setItem(CHAT_AUTO_APPROVE_KEY, enabled ? "1" : "0");
  } catch {
    // ignore storage failures
  }
}

function isPackBuilderIntent(content) {
  const lower = String(content || "").toLowerCase();
  const mentionsPack =
    lower.includes("pack") || lower.includes("automation") || lower.includes("workflow");
  const mentionsCreate =
    lower.includes("create") ||
    lower.includes("build") ||
    lower.includes("make") ||
    lower.includes("generate") ||
    lower.includes("setup");
  const mentionsExternal =
    lower.includes("notion") ||
    lower.includes("slack") ||
    lower.includes("stripe") ||
    lower.includes("mcp") ||
    lower.includes("connector") ||
    lower.includes("headline") ||
    lower.includes("news") ||
    lower.includes("email");
  return mentionsPack && mentionsCreate && mentionsExternal;
}

function parsePackBuilderReplyCommand(content) {
  const trimmed = String(content || "").trim();
  if (!trimmed) return null;
  const lower = trimmed.toLowerCase();
  if (
    [
      "ok",
      "okay",
      "yes",
      "y",
      "confirm",
      "confirmed",
      "approve",
      "approved",
      "go",
      "go ahead",
      "proceed",
      "do it",
      "run it",
      "apply",
    ].includes(lower)
  ) {
    return { mode: "confirm" };
  }
  if (["cancel", "stop", "abort"].includes(lower)) {
    return { mode: "cancel" };
  }
  if (lower.startsWith("use connectors:")) {
    const list = trimmed
      .slice(trimmed.indexOf(":") + 1)
      .split(",")
      .map((item) => item.trim())
      .filter(Boolean);
    if (list.length) return { mode: "connectors", connectors: list };
  }
  return null;
}

export async function renderChat(ctx) {
  const { state, byId, toast, escapeHtml, api, renderIcons, addCleanup, setRoute } = ctx;
  const sessions = await loadSessions();
  if (!state.currentSessionId) state.currentSessionId = sessions[0]?.id || "";
  let sessionsOpen = false;

  byId("view").innerHTML = `
    <div id="chat-layout" class="chat-layout min-w-0 h-[calc(100vh-2rem)]">
      <aside id="chat-sessions-panel" class="chat-sessions-panel">
        <div class="chat-sessions-header">
          <h3 class="chat-sessions-title"><i data-lucide="clock-3"></i> Sessions</h3>
          <button id="new-session" class="tcp-btn h-8 px-2.5 text-xs"><i data-lucide="plus"></i> New</button>
        </div>
        <div id="session-list" class="chat-session-list"></div>
      </aside>
      <button id="chat-scrim" class="chat-scrim" aria-label="Close sessions"></button>
      <div class="chat-workspace min-h-0 min-w-0">
      <div class="chat-main-shell flex min-h-0 min-w-0 flex-col overflow-hidden">
        <div class="chat-main-header shrink-0">
          <button id="chat-toggle-sessions" type="button" class="chat-icon-btn h-8 w-8" title="Sessions"><i data-lucide="clock-3"></i></button>
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
            <textarea id="chat-input" rows="1" class="tcp-input chat-input-with-clip chat-input-modern resize-none" placeholder="Ask anything... (Enter to send, Shift+Enter newline)"></textarea>
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
        <section class="min-h-0">
          <div class="mb-2 flex items-center justify-between">
            <p class="chat-rail-label">Approvals</p>
            <span id="chat-rail-permissions-count" class="chat-rail-count">0</span>
          </div>
          <div class="mb-2 flex items-center gap-2">
            <button id="chat-approve-all" class="tcp-btn h-7 px-2 text-[11px]">Approve all</button>
            <label class="chat-auto-approve-label">
              <input id="chat-auto-approve" type="checkbox" class="chat-auto-approve-checkbox" />
              Auto
            </label>
          </div>
          <div id="chat-permissions-list" class="chat-tools-activity"></div>
        </section>
        <section class="min-h-0 flex-1">
          <div class="mb-2 flex items-center justify-between">
            <p class="chat-rail-label">Pack Events</p>
            <div class="flex items-center gap-2">
              <span id="chat-pack-events-count" class="chat-rail-count">0</span>
              <button id="chat-pack-events-clear" class="tcp-btn h-7 px-2 text-[11px]">Clear</button>
            </div>
          </div>
          <div id="chat-pack-events" class="chat-tools-activity"></div>
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
  const railPermissionsCountEl = byId("chat-rail-permissions-count");
  const packEventsCountEl = byId("chat-pack-events-count");
  const packEventsClearEl = byId("chat-pack-events-clear");
  const packEventsListEl = byId("chat-pack-events");
  const permissionsListEl = byId("chat-permissions-list");
  const approveAllEl = byId("chat-approve-all");
  const autoApproveEl = byId("chat-auto-approve");
  const toolsListEl = byId("chat-tools-list");
  const toolsActivityEl = byId("chat-tools-activity");
  const uploadedFiles = Array.isArray(state.chatUploadedFiles) ? state.chatUploadedFiles : [];
  state.chatUploadedFiles = uploadedFiles;
  const uploadState = new Map();
  const toolActivity = [];
  const toolEventSeen = new Set();
  const packEvents = [];
  const packEventSeen = new Set();
  const permissionRequests = [];
  const permissionBusy = new Set();
  let autoApproveTools = loadAutoApprovePreference();
  let autoApproveInFlight = false;
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
    const isInternalProviderTestSession = (session) =>
      String(session?.title || "")
        .trim()
        .toLowerCase()
        .startsWith("__provider_test__");
    try {
      const list = await state.client.sessions.list({ pageSize: 50 });
      if (Array.isArray(list)) return list.filter((row) => !isInternalProviderTestSession(row));
      if (Array.isArray(list?.sessions))
        return list.sessions.filter((row) => !isInternalProviderTestSession(row));
    } catch {
      // Fallback below handles older/newer response shapes via raw engine endpoint.
    }
    try {
      const raw = await api("/api/engine/session?page_size=50");
      if (Array.isArray(raw)) return raw.filter((row) => !isInternalProviderTestSession(row));
      if (Array.isArray(raw?.sessions))
        return raw.sessions.filter((row) => !isInternalProviderTestSession(row));
      return [];
    } catch {
      return [];
    }
  }

  function currentModelRoute() {
    const providerID = String(state.providerDefault || "").trim();
    const modelID = String(state.providerDefaultModel || "").trim();
    if (!providerID || !modelID) return null;
    return { providerID, modelID };
  }

  async function resolveModelRoute() {
    const known = currentModelRoute();
    if (known) return known;
    try {
      const cfg = await state.client.providers.config();
      const providerID = String(cfg?.default || "").trim();
      const modelID = String(cfg?.providers?.[providerID]?.default_model || "").trim();
      if (providerID) state.providerDefault = providerID;
      if (modelID) state.providerDefaultModel = modelID;
      if (providerID && modelID) return { providerID, modelID };
    } catch {
      // Use existing state fallback below.
    }
    return currentModelRoute();
  }

  async function createSession() {
    const modelRoute = await resolveModelRoute();
    const createPayload = { title: `Chat ${new Date().toLocaleTimeString()}` };
    if (modelRoute) {
      createPayload.provider = modelRoute.providerID;
      createPayload.model = modelRoute.modelID;
    }
    const sid = await state.client.sessions.create(createPayload);
    const rec = await state.client.sessions.get(sid).catch(() => ({ id: sid, title: sid }));
    sessions.unshift(rec);
    state.currentSessionId = sid;
    resetToolTracking();
    resetPackTracking();
    renderSessions();
    await refreshPermissionRequests();
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

  function normalizePackEvent(rawType, rawProps) {
    const props = rawProps && typeof rawProps === "object" ? rawProps : {};
    const type = String(rawType || "").trim() || "pack.event";
    const path = String(props.path || "").trim();
    const attachment_id = String(props.attachment_id || props.attachmentId || "").trim();
    const connector = String(props.connector || "").trim();
    const channel_id = String(props.channel_id || props.channelId || "").trim();
    const sender_id = String(props.sender_id || props.senderId || "").trim();
    const name = String(props.name || "").trim();
    const version = String(props.version || "").trim();
    const error = String(props.error || "").trim();
    const detailBits = [];
    if (name) detailBits.push(name);
    if (version) detailBits.push(version);
    if (path) detailBits.push(path);
    if (connector) detailBits.push(connector);
    if (channel_id) detailBits.push(`channel=${channel_id}`);
    if (sender_id) detailBits.push(`sender=${sender_id}`);
    const summary = detailBits.join(" · ");
    return {
      id: `${type}:${attachment_id || path || name || "event"}`,
      type,
      path,
      attachment_id,
      connector,
      channel_id,
      sender_id,
      error,
      summary: summary || type,
      at: Date.now(),
    };
  }

  function renderPackRail() {
    if (!packEventsCountEl || !packEventsListEl) return;
    packEventsCountEl.textContent = String(packEvents.length);
    packEventsListEl.innerHTML =
      packEvents
        .slice(0, 20)
        .map((ev) => {
          const at = new Date(ev.at).toLocaleTimeString();
          return `
          <article class="chat-pack-event-card">
            <div class="flex items-center justify-between gap-2">
              <div class="chat-pack-event-title truncate" title="${escapeHtml(ev.type)}">${escapeHtml(ev.type)}</div>
              <span class="chat-pack-event-time">${escapeHtml(at)}</span>
            </div>
            <div class="chat-pack-event-summary mt-0.5">${escapeHtml(ev.summary)}</div>
            ${ev.error ? `<div class="chat-pack-event-error mt-1">${escapeHtml(ev.error)}</div>` : ""}
            <div class="mt-1 flex flex-wrap gap-1">
              <button class="tcp-btn h-6 px-1.5 text-[10px]" data-pack-open="1">Packs</button>
              ${
                ev.path
                  ? `<button class="tcp-btn h-6 px-1.5 text-[10px]" data-pack-install-path="${escapeHtml(ev.path)}">Install path</button>`
                  : ""
              }
              ${
                ev.path && ev.attachment_id
                  ? `<button class="tcp-btn h-6 px-1.5 text-[10px]" data-pack-install-attachment="${escapeHtml(ev.attachment_id)}" data-pack-path="${escapeHtml(ev.path)}" data-pack-connector="${escapeHtml(ev.connector)}" data-pack-channel="${escapeHtml(ev.channel_id)}" data-pack-sender="${escapeHtml(ev.sender_id)}">Install attach</button>`
                  : ""
              }
            </div>
          </article>
        `;
        })
        .join("") || '<p class="chat-rail-empty">No pack events yet.</p>';

    packEventsListEl.querySelectorAll("[data-pack-open]").forEach((el) => {
      el.addEventListener("click", () => {
        setRoute?.("packs");
      });
    });
    packEventsListEl.querySelectorAll("[data-pack-install-path]").forEach((el) => {
      el.addEventListener("click", async () => {
        const path = String(el.getAttribute("data-pack-install-path") || "").trim();
        if (!path) return;
        try {
          const payload = await state.client.packs.install({
            path,
            source: { kind: "control_panel_chat", event: "pack.detected" },
          });
          toast(
            "ok",
            `Installed ${payload?.installed?.name || "pack"} ${payload?.installed?.version || ""}`.trim()
          );
        } catch (e) {
          toast("err", `Install failed: ${e instanceof Error ? e.message : String(e)}`);
        }
      });
    });
    packEventsListEl.querySelectorAll("[data-pack-install-attachment]").forEach((el) => {
      el.addEventListener("click", async () => {
        const attachmentID = String(el.getAttribute("data-pack-install-attachment") || "").trim();
        const path = String(el.getAttribute("data-pack-path") || "").trim();
        if (!attachmentID || !path) return;
        try {
          const payload = await state.client.packs.installFromAttachment({
            attachment_id: attachmentID,
            path,
            connector: String(el.getAttribute("data-pack-connector") || "").trim() || undefined,
            channel_id: String(el.getAttribute("data-pack-channel") || "").trim() || undefined,
            sender_id: String(el.getAttribute("data-pack-sender") || "").trim() || undefined,
          });
          toast(
            "ok",
            `Installed ${payload?.installed?.name || "pack"} ${payload?.installed?.version || ""}`.trim()
          );
        } catch (e) {
          toast("err", `Install failed: ${e instanceof Error ? e.message : String(e)}`);
        }
      });
    });
  }

  function resetPackTracking() {
    packEvents.splice(0, packEvents.length);
    packEventSeen.clear();
    renderPackRail();
  }

  function recordPackEvent(rawType, rawProps) {
    const normalized = normalizePackEvent(rawType, rawProps);
    const lower = String(normalized.type).toLowerCase();
    if (!lower.startsWith("pack.") && !lower.startsWith("pack_builder.")) return;
    if (packEventSeen.has(normalized.id)) return;
    packEventSeen.add(normalized.id);
    if (packEventSeen.size > 400) packEventSeen.clear();
    packEvents.unshift(normalized);
    if (packEvents.length > 80) packEvents.length = 80;
    renderPackRail();
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

  function appendTransientUserMessage(text, attachedCount = 0) {
    const content = String(text || "").trim();
    if (!content) return;
    const bubble = document.createElement("div");
    bubble.className = "chat-msg user";
    bubble.innerHTML = `
      <div class="chat-msg-role">User</div>
      <pre class="chat-msg-pre">${escapeHtml(content)}</pre>
      ${
        attachedCount > 0
          ? `<div class="chat-msg-attachments mt-1">${attachedCount} attachment${attachedCount === 1 ? "" : "s"}</div>`
          : ""
      }
    `;
    messagesEl.appendChild(bubble);
    messagesEl.scrollTop = messagesEl.scrollHeight;
  }

  function appendTransientAssistantMessage(text) {
    const content = String(text || "").trim();
    if (!content) return;
    const assistantLabel = String(state.botName || "Assistant").trim() || "Assistant";
    const assistantAvatar = String(state.botAvatarUrl || "").trim();
    const bubble = document.createElement("div");
    bubble.className = "chat-msg assistant";
    bubble.innerHTML = `
      <div class="chat-msg-role">
        <span class="inline-flex items-center gap-2">
          ${
            assistantAvatar
              ? `<img src="${escapeHtml(assistantAvatar)}" alt="${escapeHtml(assistantLabel)}" class="chat-avatar-ring h-5 w-5 rounded-full object-cover" />`
              : ""
          }
          <span>${escapeHtml(assistantLabel)}</span>
        </span>
      </div>
      <div class="tcp-markdown tcp-markdown-ai">${renderMarkdown(content)}</div>
    `;
    messagesEl.appendChild(bubble);
    messagesEl.scrollTop = messagesEl.scrollHeight;
  }

  function normalizeToolName(value) {
    return String(value || "")
      .trim()
      .replace(/\s+/g, " ")
      .replace(/[<>]/g, "");
  }

  function extractToolName(payload) {
    const source = payload || {};
    const nested = source.call || source.toolCall || source.part || {};
    return normalizeToolName(
      source.tool ||
        source.name ||
        source.toolName ||
        source.tool_id ||
        source.toolID ||
        nested.tool ||
        nested.name ||
        nested.toolName ||
        ""
    );
  }

  function extractToolCallId(payload) {
    const source = payload || {};
    const nested = source.call || source.toolCall || source.part || {};
    return String(
      source.callID ||
        source.toolCallID ||
        source.tool_call_id ||
        source.id ||
        nested.callID ||
        nested.toolCallID ||
        nested.tool_call_id ||
        nested.id ||
        ""
    ).trim();
  }

  function extractRunId(event, fallback = "") {
    const props = event?.properties || {};
    return String(
      event?.runId ||
        event?.runID ||
        event?.run_id ||
        props.runID ||
        props.runId ||
        props.run_id ||
        props.run?.id ||
        fallback
    ).trim();
  }

  function normalizePermissionRequest(raw) {
    if (!raw) return null;
    const nested = raw.request || raw.approval || raw.permission || {};
    const id = String(
      raw.id ||
        raw.requestID ||
        raw.requestId ||
        raw.approvalID ||
        nested.id ||
        nested.requestID ||
        nested.requestId ||
        ""
    ).trim();
    if (!id) return null;
    const sessionId = String(
      raw.sessionId ||
        raw.sessionID ||
        raw.session_id ||
        nested.sessionId ||
        nested.sessionID ||
        nested.session_id ||
        ""
    ).trim();
    return {
      id,
      tool: normalizeToolName(raw.tool || nested.tool || nested.name || "tool") || "tool",
      permission: String(raw.permission || nested.permission || "").trim(),
      pattern: String(raw.pattern || nested.pattern || "").trim(),
      sessionId,
      status: String(raw.status || nested.status || "").trim().toLowerCase(),
    };
  }

  function isPendingPermissionStatus(statusRaw) {
    const status = String(statusRaw || "").trim().toLowerCase();
    if (!status) return true;
    if (
      status.includes("approved") ||
      status.includes("rejected") ||
      status.includes("denied") ||
      status.includes("resolved") ||
      status.includes("expired") ||
      status.includes("cancel") ||
      status.includes("complete") ||
      status.includes("done") ||
      status.includes("timeout")
    ) {
      return false;
    }
    return (
      status.includes("pending") ||
      status.includes("request") ||
      status.includes("ask") ||
      status.includes("await") ||
      status.includes("open") ||
      status.includes("queue") ||
      status.includes("new") ||
      status.includes("progress") ||
      status === "unknown"
    );
  }

  function upsertPermissionRequest(req) {
    if (!isPendingPermissionStatus(req?.status)) return;
    const idx = permissionRequests.findIndex((item) => item.id === req.id);
    if (idx >= 0) permissionRequests[idx] = { ...permissionRequests[idx], ...req };
    else permissionRequests.unshift(req);
  }

  function removePermissionRequest(requestId) {
    const idx = permissionRequests.findIndex((item) => item.id === requestId);
    if (idx >= 0) permissionRequests.splice(idx, 1);
  }

  function renderPermissionRail() {
    const count = permissionRequests.length;
    railPermissionsCountEl.textContent = String(count);
    approveAllEl.disabled = count === 0 || autoApproveInFlight;
    autoApproveEl.checked = autoApproveTools;
    permissionsListEl.innerHTML =
      permissionRequests
        .slice(0, 20)
        .map((req) => {
          const busy = permissionBusy.has(req.id);
          const bits = [req.permission, req.pattern].filter(Boolean).join(" ");
          return `
            <article class="chat-pack-event-card">
              <div class="chat-pack-event-title truncate" title="${escapeHtml(req.id)}">${escapeHtml(req.tool)}</div>
              <div class="chat-pack-event-summary mt-0.5">${escapeHtml(bits || req.id)}</div>
              <div class="mt-1 flex gap-1">
                <button class="tcp-btn h-6 px-1.5 text-[10px]" data-perm-allow="${escapeHtml(req.id)}" ${busy ? "disabled" : ""}>Allow</button>
                <button class="tcp-btn h-6 px-1.5 text-[10px]" data-perm-always="${escapeHtml(req.id)}" ${busy ? "disabled" : ""}>Always</button>
                <button class="tcp-btn-danger h-6 px-1.5 text-[10px]" data-perm-deny="${escapeHtml(req.id)}" ${busy ? "disabled" : ""}>Deny</button>
              </div>
            </article>
          `;
        })
        .join("") || '<p class="chat-rail-empty">No pending approvals.</p>';

    permissionsListEl.querySelectorAll("[data-perm-allow]").forEach((el) => {
      el.addEventListener("click", async () => {
        const requestId = String(el.dataset.permAllow || "").trim();
        if (!requestId) return;
        await replyPermission(requestId, "once");
      });
    });
    permissionsListEl.querySelectorAll("[data-perm-always]").forEach((el) => {
      el.addEventListener("click", async () => {
        const requestId = String(el.dataset.permAlways || "").trim();
        if (!requestId) return;
        await replyPermission(requestId, "always");
      });
    });
    permissionsListEl.querySelectorAll("[data-perm-deny]").forEach((el) => {
      el.addEventListener("click", async () => {
        const requestId = String(el.dataset.permDeny || "").trim();
        if (!requestId) return;
        await replyPermission(requestId, "deny");
      });
    });
  }

  async function replyPermission(requestId, replyMode, quiet = false) {
    if (!requestId || permissionBusy.has(requestId)) return;
    const normalizedReply = replyMode === "allow" ? "once" : replyMode;
    permissionBusy.add(requestId);
    renderPermissionRail();
    try {
      await state.client.permissions.reply(requestId, normalizedReply);
      removePermissionRequest(requestId);
      if (!quiet) {
        toast(
          "ok",
          `Permission ${normalizedReply === "deny" ? "denied" : "approved"} (${requestId}).`
        );
      }
    } catch (e) {
      if (!quiet) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    } finally {
      permissionBusy.delete(requestId);
      renderPermissionRail();
      void refreshPermissionRequests();
    }
  }

  async function autoApprovePendingRequests() {
    if (!autoApproveTools || autoApproveInFlight || permissionRequests.length === 0) return;
    autoApproveInFlight = true;
    renderPermissionRail();
    try {
      for (const req of [...permissionRequests]) {
        await replyPermission(req.id, "always", true);
      }
    } finally {
      autoApproveInFlight = false;
      renderPermissionRail();
    }
  }

  async function refreshPermissionRequests() {
    const snapshot = await state.client.permissions.list().catch(() => ({ requests: [] }));
    const list = Array.isArray(snapshot?.requests) ? snapshot.requests : [];
    const sessionId = String(state.currentSessionId || "").trim();
    permissionRequests.splice(0, permissionRequests.length);
    for (const raw of list) {
      const normalized = normalizePermissionRequest(raw);
      if (!normalized) continue;
      if (normalized.sessionId && sessionId && normalized.sessionId !== sessionId) continue;
      if (!isPendingPermissionStatus(normalized.status)) continue;
      permissionRequests.push(normalized);
    }
    renderPermissionRail();
    void autoApprovePendingRequests();
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
          <div class="chat-upload-card">
            <div class="mb-1 flex items-center justify-between gap-2 text-xs">
              <span class="chat-upload-name truncate">${escapeHtml(item.name)}</span>
              <span class="${item.error ? "chat-upload-meta-error" : "chat-upload-meta"}">${item.error ? escapeHtml(item.error) : `${pct}%`}</span>
            </div>
            <div class="chat-upload-bar">
              <div class="chat-upload-bar-fill" style="width:${pct}%"></div>
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
    resetPackTracking();
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
        resetPackTracking();
        renderSessions();
        await refreshPermissionRequests();
        await renderMessages();
        setSessionsPanel(false);
      });
    });

    listEl.querySelectorAll("[data-del-sid]").forEach((btn) => {
      btn.addEventListener("click", async (e) => {
        e.stopPropagation();
        const sid = btn.dataset.delSid;
        if (!sid) return;
        const approved = await confirmActionModal({
          title: "Delete session",
          message: "This will permanently remove this chat session and its messages.",
          confirmLabel: "Delete session",
        });
        if (!approved) return;
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
    const assistantLabel = String(state.botName || "Assistant").trim() || "Assistant";
    const assistantAvatar = String(state.botAvatarUrl || "").trim();
    messagesEl.innerHTML = messages
      .map((m) => {
        const roleRaw = String(m?.info?.role || "unknown");
        const displayRole =
          roleRaw === "assistant"
            ? assistantLabel
            : roleRaw === "user"
              ? "User"
              : roleRaw === "system"
                ? "System"
                : roleRaw;
        const role = escapeHtml(displayRole);
        const roleHtml =
          roleRaw === "assistant"
            ? `<span class="inline-flex items-center gap-2">${assistantAvatar ? `<img src="${escapeHtml(assistantAvatar)}" alt="${escapeHtml(assistantLabel)}" class="chat-avatar-ring h-5 w-5 rounded-full object-cover" />` : ""}<span>${role}</span></span>`
            : role;
        const textRaw = (m.parts || []).map((p) => p.text || "").join("\n");
        const isAssistantLike = roleRaw === "assistant" || roleRaw === "system";
        const content = isAssistantLike
          ? `<div class="tcp-markdown tcp-markdown-ai">${renderMarkdown(textRaw)}</div>`
          : `<pre class="chat-msg-pre">${escapeHtml(textRaw)}</pre>`;
        const roleClass = isAssistantLike ? "assistant" : "user";
        return `<div class="chat-msg ${roleClass}"><div class="chat-msg-role">${roleHtml}</div>${content}</div>`;
      })
      .join("");

    messagesEl.scrollTop = messagesEl.scrollHeight;
  }

  byId("new-session").addEventListener("click", async () => {
    setSessionsPanel(false);
    try {
      await createSession();
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });
  approveAllEl?.addEventListener("click", async () => {
    const pendingIds = permissionRequests.map((req) => req.id).filter(Boolean);
    if (!pendingIds.length) return;
    for (const requestId of pendingIds) {
      // Approve once for currently pending requests.
      await replyPermission(requestId, "once", true);
    }
    await refreshPermissionRequests();
    const unresolved = pendingIds.filter((id) =>
      permissionRequests.some((req) => String(req.id || "").trim() === id)
    ).length;
    if (unresolved > 0) {
      toast(
        "warn",
        `${unresolved} request${unresolved === 1 ? "" : "s"} still pending (likely stale/expired).`
      );
    } else {
      toast(
        "ok",
        `Approved ${pendingIds.length} pending request${pendingIds.length === 1 ? "" : "s"}.`
      );
    }
  });
  autoApproveEl?.addEventListener("change", () => {
    autoApproveTools = !!autoApproveEl.checked;
    saveAutoApprovePreference(autoApproveTools);
    renderPermissionRail();
    if (autoApproveTools) void autoApprovePendingRequests();
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
      appendTransientUserMessage(prompt, attached.length);
      const threadKey = `control-panel:${String(state.currentSessionId || "").trim()}`;
      const replyCommand = parsePackBuilderReplyCommand(promptRaw);
      if (replyCommand || isPackBuilderIntent(promptRaw)) {
        let payload = null;
        if (replyCommand?.mode === "cancel") {
          payload = await api("/api/engine/pack-builder/cancel", {
            method: "POST",
            body: JSON.stringify({
              session_id: state.currentSessionId,
              thread_key: threadKey,
            }),
          });
        } else if (replyCommand?.mode === "confirm" || replyCommand?.mode === "connectors") {
          const pending = await api(
            `/api/engine/pack-builder/pending?session_id=${encodeURIComponent(state.currentSessionId)}&thread_key=${encodeURIComponent(threadKey)}`,
            { method: "GET" }
          );
          const planId = String(pending?.pending?.plan_id || pending?.plan_id || "").trim();
          if (!planId) {
            throw new Error("No pending pack-builder plan for this chat thread.");
          }
          payload = await api("/api/engine/pack-builder/apply", {
            method: "POST",
            body: JSON.stringify({
              plan_id: planId,
              session_id: state.currentSessionId,
              thread_key: threadKey,
              selected_connectors: replyCommand?.mode === "connectors" ? replyCommand.connectors : [],
              approvals: {
                approve_connector_registration: true,
                approve_pack_install: true,
                approve_enable_routines: false,
              },
              secret_refs_confirmed: replyCommand?.mode === "connectors",
            }),
          });
        } else {
          payload = await api("/api/engine/pack-builder/preview", {
            method: "POST",
            body: JSON.stringify({
              goal: promptRaw,
              session_id: state.currentSessionId,
              thread_key: threadKey,
              auto_apply: false,
            }),
          });
        }
        const summary = String(payload?.output || "").trim() || JSON.stringify(payload || {}, null, 2);
        appendTransientAssistantMessage(summary);
        if (attached.length > 0) {
          uploadedFiles.splice(0, uploadedFiles.length);
          renderUploadedFiles();
        }
        return;
      }
      const modelRoute = await resolveModelRoute();
      if (!modelRoute) {
        throw new Error(
          "No default provider/model configured. Set it in Settings before sending chat."
        );
      }
      if (attached.length > 0) {
        toast(
          "info",
          `Sending with ${attached.length} attached file${attached.length === 1 ? "" : "s"}.`
        );
      }
      const parts = attached.map((f) => ({
        type: "file",
        mime: f.mime || inferMime(f.name || f.path),
        filename: f.name || f.path || "attachment",
        url: f.url || f.path,
      }));
      parts.push({ type: "text", text: prompt });

      const getActiveRunId = async () => {
        const res = await fetch(
          `/api/engine/session/${encodeURIComponent(state.currentSessionId)}/run`,
          {
            method: "GET",
            credentials: "include",
          }
        );
        if (!res.ok) return "";
        const payload = await res.json().catch(() => ({}));
        return payload?.active?.runID || payload?.active?.runId || payload?.active?.run_id || "";
      };

      const cancelAndWaitForIdle = async () => {
        const activeRunId = await getActiveRunId().catch(() => "");
        if (activeRunId) {
          await fetch(
            `/api/engine/session/${encodeURIComponent(state.currentSessionId)}/run/${encodeURIComponent(activeRunId)}/cancel`,
            {
              method: "POST",
              credentials: "include",
              headers: { "content-type": "application/json" },
              body: JSON.stringify({}),
            }
          ).catch(() => {});
        }
        await fetch(`/api/engine/session/${encodeURIComponent(state.currentSessionId)}/cancel`, {
          method: "POST",
          credentials: "include",
          headers: { "content-type": "application/json" },
          body: JSON.stringify({}),
        }).catch(() => {});
        for (let i = 0; i < 50; i += 1) {
          const active = await getActiveRunId().catch(() => "");
          if (!active) return true;
          await new Promise((resolve) => setTimeout(resolve, 200));
        }
        return false;
      };

      const startRun = async () =>
        fetch(
          `/api/engine/session/${encodeURIComponent(state.currentSessionId)}/prompt_async?return=run`,
          {
            method: "POST",
            credentials: "include",
            headers: { "content-type": "application/json" },
            body: JSON.stringify({
              parts,
              model: {
                providerID: modelRoute.providerID,
                modelID: modelRoute.modelID,
              },
            }),
          }
        );

      let runResp = await startRun();
      let runId = "";
      if (runResp.status === 409) {
        const becameIdle = await cancelAndWaitForIdle();
        if (!becameIdle) {
          throw new Error(
            "Session has a stuck active run. Cancel it from engine/session and retry."
          );
        }
        runResp = await startRun();
        if (runResp.ok) {
          const retryPayload = await runResp.json().catch(() => ({}));
          runId = retryPayload?.runID || retryPayload?.runId || retryPayload?.run_id || "";
        } else if (runResp.status === 409) {
          throw new Error("Session is still busy with another run. Please retry in a moment.");
        } else {
          const body = await runResp.text().catch(() => "");
          throw new Error(`prompt_async retry failed (${runResp.status}): ${body}`);
        }
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
      const assistantLabel = escapeHtml(String(state.botName || "Assistant").trim() || "Assistant");
      const assistantAvatar = String(state.botAvatarUrl || "").trim();
      const placeholder = document.createElement("div");
      placeholder.className = "chat-msg assistant";
      placeholder.innerHTML = `
        <div class="chat-msg-role"><span class="inline-flex items-center gap-2">${assistantAvatar ? `<img src="${escapeHtml(assistantAvatar)}" alt="${assistantLabel}" class="chat-avatar-ring h-5 w-5 rounded-full object-cover" />` : ""}<span>${assistantLabel}</span></span></div>
        <div class="tcp-thinking" aria-live="polite">
          <span>Thinking</span>
          <i></i><i></i><i></i>
        </div>
        <pre class="streaming-msg chat-msg-pre hidden"></pre>
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
      const toolStartEvents = new Set(["tool.called", "tool_call.started", "session.tool_call"]);
      const toolEndEvents = new Set([
        "tool.result",
        "tool_call.completed",
        "tool_call.failed",
        "session.tool_result",
      ]);

      let streamTimedOut = false;
      const streamAbort = new AbortController();
      let noEventTimer = null;
      let maxStreamTimer = null;
      let streamAbortReason = "";
      const NO_EVENT_TIMEOUT_MS = 30000;
      const MAX_STREAM_WINDOW_MS = 180000;
      const waitForRunToSettle = async (targetRunId, timeoutMs) => {
        const startedAt = Date.now();
        while (Date.now() - startedAt < timeoutMs) {
          const active = await getActiveRunId().catch(() => targetRunId);
          await renderMessages();
          if (!active || active !== targetRunId) return true;
          await new Promise((resolve) => setTimeout(resolve, 350));
        }
        return false;
      };
      const isRunSignalEvent = (eventType) => {
        const t = String(eventType || "").trim();
        return t !== "server.connected" && t !== "engine.lifecycle.ready";
      };
      const resetNoEventTimer = () => {
        if (noEventTimer) clearTimeout(noEventTimer);
        noEventTimer = setTimeout(() => {
          streamTimedOut = true;
          streamAbortReason = "no-events-timeout";
          streamAbort.abort("no-events-timeout");
        }, NO_EVENT_TIMEOUT_MS);
      };
      resetNoEventTimer();
      maxStreamTimer = setTimeout(() => {
        streamTimedOut = true;
        streamAbortReason = "max-stream-window";
        streamAbort.abort("max-stream-window");
      }, MAX_STREAM_WINDOW_MS);

      try {
        for await (const event of state.client.stream(state.currentSessionId, runId, {
          signal: streamAbort.signal,
        })) {
          if (isRunSignalEvent(event.type)) {
            resetNoEventTimer();
          }
          const evRunId = extractRunId(event);
          if (
            event.type === "approval.requested" ||
            event.type === "permission.request" ||
            event.type === "permission.asked"
          ) {
            const req = normalizePermissionRequest(event.properties || {});
            if (req) {
              if (!req.sessionId || req.sessionId === String(state.currentSessionId || "").trim()) {
                upsertPermissionRequest(req);
                renderPermissionRail();
                void autoApprovePendingRequests();
              }
            } else {
              void refreshPermissionRequests();
            }
          }
          if (
            event.type === "approval.resolved" ||
            event.type === "permission.resolved" ||
            event.type === "permission.replied"
          ) {
            const req = normalizePermissionRequest(event.properties || {});
            if (req?.id) removePermissionRequest(req.id);
            renderPermissionRail();
            void refreshPermissionRequests();
          }
          if (String(event.type || "").toLowerCase().startsWith("pack.")) {
            recordPackEvent(event.type, event.properties || {});
          }
          if (String(event.type || "").toLowerCase().startsWith("pack_builder.")) {
            recordPackEvent(event.type, event.properties || {});
          }
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
          if (toolStartEvents.has(event.type)) {
            const tool = extractToolName(event.properties) || "tool";
            const callId = extractToolCallId(event.properties);
            recordToolActivity(
              tool,
              "started",
              `${callId || evRunId || runId}:${tool}:started`
            );
          }
          if (toolEndEvents.has(event.type)) {
            const tool = extractToolName(event.properties) || "tool";
            const callId = extractToolCallId(event.properties);
            const statusHint = String(
              event.properties?.status || event.properties?.state || ""
            ).toLowerCase();
            const failed =
              event.type === "tool_call.failed" ||
              statusHint.includes("fail") ||
              statusHint.includes("error") ||
              !!event.properties?.error;
            recordToolActivity(
              tool,
              failed ? "failed" : "completed",
              `${callId || evRunId || runId}:${tool}:${failed ? "failed" : "completed"}`
            );
          }
          if (event.type === "message.part.updated") {
            const part = event.properties?.part || {};
            const partType = String(part.type || "")
              .trim()
              .toLowerCase()
              .replace(/_/g, "-");
            const tool = extractToolName(part) || extractToolName(event.properties);
            const partId = extractToolCallId(part);
            const partState = part?.state;
            const partStatus = String(
              (partState && typeof partState === "object" ? partState.status : partState) ||
                part.status ||
                ""
            )
              .trim()
              .toLowerCase();
            const hasError =
              !!part.error ||
              !!(partState && typeof partState === "object" && partState.error) ||
              partStatus.includes("fail") ||
              partStatus.includes("error") ||
              partStatus.includes("deny") ||
              partStatus.includes("reject") ||
              partStatus.includes("cancel");
            const hasOutput =
              !!part.result ||
              !!part.output ||
              !!(partState && typeof partState === "object" && (partState.output || partState.result)) ||
              partStatus.includes("done") ||
              partStatus.includes("complete") ||
              partStatus.includes("success");
            if (tool && (partType === "tool" || partType === "tool-invocation")) {
              const status = hasError ? "failed" : hasOutput ? "completed" : "started";
              recordToolActivity(
                tool,
                status,
                `${partId || evRunId || runId}:${tool}:${status}`
              );
            }
            if (tool && partType === "tool-result") {
              recordToolActivity(
                tool,
                hasError ? "failed" : "completed",
                `${partId || evRunId || runId}:${tool}:${hasError ? "failed" : "completed"}`
              );
            }
          }
          if (terminalFailureEvents.has(event.type)) {
            throw new Error(String(event.properties?.error || "Run failed."));
          }
          if (
            (event.type === "session.updated" || event.type === "session.status") &&
            String(event.properties?.status || "").toLowerCase() === "idle"
          ) {
            break;
          }
          if (terminalSuccessEvents.has(event.type)) {
            break;
          }
        }
      } catch (streamErr) {
        const errText = String(streamErr?.message || streamErr || "").toLowerCase();
        const isAbortLike =
          streamTimedOut ||
          errText.includes("abort") ||
          errText.includes("terminated") ||
          errText.includes("networkerror");
        if (!isAbortLike) throw streamErr;
      }
      if (noEventTimer) clearTimeout(noEventTimer);
      if (maxStreamTimer) clearTimeout(maxStreamTimer);

      if (streamTimedOut) {
        // Fallback: give the run time to settle before declaring failure.
        const settled = await waitForRunToSettle(runId, 45000);
        await renderMessages();
        if (!settled) {
          throw new Error(
            "Run stream timed out and the run is still active. Check engine/provider logs and retry."
          );
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

      if (!gotDelta) {
        const activeAfter = await getActiveRunId().catch(() => "");
        if (activeAfter === runId) {
          const settled = await waitForRunToSettle(runId, 30000);
          if (settled) {
            await renderMessages();
            return;
          }
          const reason = streamAbortReason || "stream-ended-without-final-delta";
          throw new Error(`Run ${runId} is still active without a final response (${reason}).`);
        }
      }
    } catch (e) {
      const rawMsg = e instanceof Error ? e.message : String(e);
      const msg =
        rawMsg.includes("no-events-timeout") ||
        rawMsg.includes("max-stream-window") ||
        rawMsg.includes("AbortError") ||
        rawMsg.toLowerCase().includes("terminated")
          ? "Run stream timed out before events were received. Check engine/provider logs and retry."
          : rawMsg;
      toast("err", msg);
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
  packEventsClearEl?.addEventListener("click", () => {
    resetPackTracking();
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
  renderPackRail();
  renderPermissionRail();
  void refreshAvailableTools();
  void refreshPermissionRequests();
  const permissionPoll = setInterval(() => {
    if (state.route === "chat") void refreshPermissionRequests();
  }, 2500);
  addCleanup?.(() => clearInterval(permissionPoll));
  if (!state.currentSessionId && sessions.length === 0) {
    await createSession().catch(() => {});
  }
  setSessionsPanel(false);
  autosizeInput();
  await renderMessages();
}
