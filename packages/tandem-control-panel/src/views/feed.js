import { subscribeSse } from "../services/sse.js";

function eventTypeOf(data) {
  return data?.type || data?.event || "event";
}

function statusClassForEvent(type) {
  const t = String(type || "").toLowerCase();
  if (t.includes("fail") || t.includes("error")) return "tcp-badge-err";
  if (t.includes("warn") || t.includes("retry")) return "tcp-badge-warn";
  return "tcp-badge-info";
}

function normalizePackEvent(data) {
  const props = data?.properties && typeof data.properties === "object" ? data.properties : {};
  return {
    type: eventTypeOf(data),
    path: String(props.path || data?.path || "").trim(),
    attachment_id: String(props.attachment_id || data?.attachment_id || "").trim(),
    connector: String(props.connector || data?.connector || "").trim(),
    channel_id: String(props.channel_id || data?.channel_id || "").trim(),
    sender_id: String(props.sender_id || data?.sender_id || "").trim(),
    pack_id: String(props.pack_id || data?.pack_id || "").trim(),
    name: String(props.name || data?.name || "").trim(),
    version: String(props.version || data?.version || "").trim(),
    error: String(props.error || data?.error || "").trim(),
  };
}

function renderPackEventDetails(event, escapeHtml) {
  const details = [];
  if (event.name) details.push(`name: ${escapeHtml(event.name)}`);
  if (event.version) details.push(`version: ${escapeHtml(event.version)}`);
  if (event.pack_id) details.push(`pack_id: ${escapeHtml(event.pack_id)}`);
  if (event.path) details.push(`path: ${escapeHtml(event.path)}`);
  if (event.connector) details.push(`connector: ${escapeHtml(event.connector)}`);
  if (event.channel_id) details.push(`channel: ${escapeHtml(event.channel_id)}`);
  if (event.sender_id) details.push(`sender: ${escapeHtml(event.sender_id)}`);
  return details.join(" · ");
}

export async function renderFeed(ctx) {
  const { byId, escapeHtml, state, addCleanup, toast, setRoute } = ctx;
  byId("view").innerHTML = `
    <div class="tcp-card">
      <div class="mb-3 flex flex-wrap items-center justify-between gap-3">
        <h3 class="tcp-title flex items-center gap-2"><i data-lucide="activity"></i> Global Live Feed</h3>
        <div class="flex gap-2">
          <input id="feed-filter" class="tcp-input min-w-[220px]" placeholder="Filter by type or payload" />
          <button id="feed-clear" class="tcp-btn">Clear</button>
        </div>
      </div>
      <div id="feed-events" class="grid max-h-[68vh] gap-2 overflow-auto rounded-xl border border-slate-700 bg-black/20 p-2"></div>
    </div>
  `;

  const host = byId("feed-events");
  const events = [];

  function renderEvents() {
    const term = byId("feed-filter").value.trim().toLowerCase();
    const filtered = events.filter((x) => {
      if (!term) return true;
      const hay = `${eventTypeOf(x.data)} ${JSON.stringify(x.data || {})}`.toLowerCase();
      return hay.includes(term);
    });

    host.innerHTML =
      filtered
        .map((x) => {
          const type = eventTypeOf(x.data);
          if (String(type).toLowerCase().startsWith("pack.")) {
            const pack = normalizePackEvent(x.data);
            const summary = renderPackEventDetails(pack, escapeHtml);
            return `
          <article class="tcp-list-item">
            <div class="flex items-center justify-between gap-2">
              <strong>${escapeHtml(type)}</strong>
              <span class="${statusClassForEvent(type)}">${new Date(x.at).toLocaleTimeString()}</span>
            </div>
            <div class="tcp-subtle mt-1">${summary || "pack lifecycle event"}</div>
            ${
              pack.error
                ? `<div class="mt-1 text-xs text-rose-300">${escapeHtml(pack.error)}</div>`
                : ""
            }
            <div class="mt-2 flex flex-wrap gap-2">
              <button class="tcp-btn" data-pack-open="1">Open Pack Library</button>
              ${
                pack.path
                  ? `<button class="tcp-btn" data-pack-install-path="${escapeHtml(pack.path)}">Install from Path</button>`
                  : ""
              }
              ${
                pack.path && pack.attachment_id
                  ? `<button class="tcp-btn" data-pack-install-attachment="${escapeHtml(pack.attachment_id)}" data-pack-path="${escapeHtml(pack.path)}" data-pack-connector="${escapeHtml(pack.connector)}" data-pack-channel="${escapeHtml(pack.channel_id)}" data-pack-sender="${escapeHtml(pack.sender_id)}">Install from Attachment</button>`
                  : ""
              }
            </div>
            <details class="mt-2">
              <summary class="cursor-pointer text-xs text-slate-400">Payload</summary>
              <pre class="tcp-code mt-2">${escapeHtml(JSON.stringify(x.data, null, 2))}</pre>
            </details>
          </article>
        `;
          }
          return `
          <article class="tcp-list-item">
            <div class="flex items-center justify-between gap-2">
              <strong>${escapeHtml(type)}</strong>
              <span class="${statusClassForEvent(type)}">${new Date(x.at).toLocaleTimeString()}</span>
            </div>
            <div class="tcp-subtle mt-1">session: ${escapeHtml(x.data?.sessionID || x.data?.sessionId || "n/a")} run: ${escapeHtml(x.data?.runID || x.data?.runId || "n/a")}</div>
            <details class="mt-2">
              <summary class="cursor-pointer text-xs text-slate-400">Payload</summary>
              <pre class="tcp-code mt-2">${escapeHtml(JSON.stringify(x.data, null, 2))}</pre>
            </details>
          </article>
        `;
        })
        .join("") || '<p class="tcp-subtle">No events yet.</p>';

    host.querySelectorAll("[data-pack-open]").forEach((btn) => {
      btn.addEventListener("click", () => {
        setRoute?.("packs");
      });
    });
    host.querySelectorAll("[data-pack-install-path]").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const path = String(btn.getAttribute("data-pack-install-path") || "").trim();
        if (!path) return;
        try {
          const payload = await state.client.packs.install({
            path,
            source: { kind: "control_panel_feed", event: "pack.detected" },
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
    host.querySelectorAll("[data-pack-install-attachment]").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const attachmentID = String(btn.getAttribute("data-pack-install-attachment") || "").trim();
        const path = String(btn.getAttribute("data-pack-path") || "").trim();
        if (!attachmentID || !path) return;
        try {
          const payload = await state.client.packs.installFromAttachment({
            attachment_id: attachmentID,
            path,
            connector: String(btn.getAttribute("data-pack-connector") || "").trim() || undefined,
            channel_id: String(btn.getAttribute("data-pack-channel") || "").trim() || undefined,
            sender_id: String(btn.getAttribute("data-pack-sender") || "").trim() || undefined,
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

    host.scrollTop = host.scrollHeight;
  }

  const stopEvt = subscribeSse(
    "/api/engine/global/event",
    (e) => {
      try {
        const data = JSON.parse(e.data);
        events.push({ at: Date.now(), data });
        while (events.length > 300) events.shift();
        if (state.route === "feed") renderEvents();
      } catch {
        // ignore
      }
    },
    {
      onError: () => {
        if (state.route === "feed") toast("err", "Live feed disconnected.");
      },
    }
  );

  byId("feed-filter").addEventListener("input", renderEvents);
  byId("feed-clear").addEventListener("click", () => {
    events.length = 0;
    renderEvents();
  });

  addCleanup(stopEvt);
  renderEvents();
}
