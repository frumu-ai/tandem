function eventTypeOf(data) {
  return data?.type || data?.event || "event";
}

function statusClassForEvent(type) {
  const t = String(type || "").toLowerCase();
  if (t.includes("fail") || t.includes("error")) return "tcp-badge-err";
  if (t.includes("warn") || t.includes("retry")) return "tcp-badge-warn";
  return "tcp-badge-info";
}

export async function renderFeed(ctx) {
  const { byId, escapeHtml, state, addCleanup, toast } = ctx;
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

    host.scrollTop = host.scrollHeight;
  }

  const evt = new EventSource("/api/engine/global/event", { withCredentials: true });
  evt.onmessage = (e) => {
    try {
      const data = JSON.parse(e.data);
      events.push({ at: Date.now(), data });
      while (events.length > 300) events.shift();
      if (state.route === "feed") renderEvents();
    } catch {
      // ignore
    }
  };

  evt.onerror = () => {
    evt.close();
    toast("err", "Live feed disconnected.");
  };

  byId("feed-filter").addEventListener("input", renderEvents);
  byId("feed-clear").addEventListener("click", () => {
    events.length = 0;
    renderEvents();
  });

  addCleanup(() => evt.close());
  renderEvents();
}
