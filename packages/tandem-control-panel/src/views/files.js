function formatBytes(bytes) {
  const n = Number(bytes || 0);
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

function formatTime(ts) {
  const n = Number(ts || 0);
  if (!n) return "n/a";
  return new Date(n).toLocaleString();
}

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

function normalizeFilesPayload(raw) {
  const root = String(raw?.root || "").trim();
  const src = Array.isArray(raw)
    ? raw
    : Array.isArray(raw?.files)
      ? raw.files
      : Array.isArray(raw?.entries)
        ? raw.entries
        : Array.isArray(raw?.items)
          ? raw.items
          : [];
  return src
    .map((item) => {
      if (typeof item === "string") {
        const path = item.replace(/^\/+/, "");
        const name = path.split("/").pop() || path;
        return { name, path, size: 0, updatedAt: 0 };
      }
      const path = String(item.relative_to_base || item.path || item.key || item.name || "").replace(
        /^\/+/,
        ""
      );
      if (!path) return null;
      const name = String(item.name || path.split("/").pop() || path);
      return {
        name,
        path,
        size: Number(item.size || item.size_bytes || item.bytes || 0),
        updatedAt: Number(item.updatedAt || item.modified_at_ms || item.modifiedAt || item.mtimeMs || 0),
        mime: inferMime(name),
        url: String(item.absPath || item.absolute_path || joinRootAndRel(root, path) || path),
      };
    })
    .filter(Boolean);
}

function normalizeDir(value) {
  return String(value || "")
    .trim()
    .replace(/\\/g, "/")
    .replace(/^\/+/, "")
    .replace(/\/+$/, "");
}

const FILES_SCOPE = "control-panel";
const ENGINE_UPLOADS_PREFIX = "channel_uploads";

function ensureScopedDir(value) {
  const dir = normalizeDir(value || FILES_SCOPE);
  if (!FILES_SCOPE) return dir;
  if (dir === FILES_SCOPE || dir.startsWith(`${FILES_SCOPE}/`)) return dir;
  return FILES_SCOPE;
}

export async function renderFiles(ctx) {
  const { state, byId, api, escapeHtml, toast, renderIcons, setRoute } = ctx;
  const uploadState = new Map();
  const files = [];
  const currentDir = ensureScopedDir(state.filesDir || FILES_SCOPE);
  state.filesDir = currentDir;

  byId("view").innerHTML = `
    <div class="tcp-card grid gap-3">
      <div class="flex flex-wrap items-center justify-between gap-2">
        <h3 class="tcp-title">Storage Browser</h3>
        <div class="flex flex-wrap items-center gap-2">
          <label for="files-upload-input" class="tcp-btn cursor-pointer"><i data-lucide="file-up"></i> Upload Files</label>
          <input id="files-upload-input" type="file" class="hidden" multiple />
          <button id="files-refresh" class="tcp-btn"><i data-lucide="refresh-cw"></i> Refresh</button>
        </div>
      </div>
      <div id="files-upload-progress" class="grid gap-1.5"></div>
      <div class="overflow-auto rounded-xl border border-slate-700 bg-black/20">
        <table class="w-full min-w-[760px] text-sm">
          <thead class="border-b border-slate-700 bg-slate-900/40 text-left text-slate-400">
            <tr>
              <th class="px-3 py-2">Name</th>
              <th class="px-3 py-2">Path</th>
              <th class="px-3 py-2">Size</th>
              <th class="px-3 py-2">Updated</th>
              <th class="px-3 py-2 text-right">Actions</th>
            </tr>
          </thead>
          <tbody id="files-table"></tbody>
        </table>
      </div>
    </div>
  `;

  const tableEl = byId("files-table");
  const progressEl = byId("files-upload-progress");
  const uploadInputEl = byId("files-upload-input");

  function renderProgress() {
    const rows = [...uploadState.entries()];
    progressEl.innerHTML = rows
      .map(([_, item]) => {
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

  function renderTable() {
    if (!files.length) {
      tableEl.innerHTML = '<tr><td colspan="5" class="px-3 py-5 text-center text-slate-400">No files in this directory.</td></tr>';
      return;
    }
    tableEl.innerHTML = files
      .map(
        (f, idx) => `
          <tr class="border-b border-slate-800/70">
            <td class="px-3 py-2 font-medium text-slate-200">
              ${escapeHtml(f.name)}
            </td>
            <td class="px-3 py-2 font-mono text-xs text-slate-400">${escapeHtml(f.path)}</td>
            <td class="px-3 py-2 text-slate-300">${escapeHtml(formatBytes(f.size))}</td>
            <td class="px-3 py-2 text-slate-400">${escapeHtml(formatTime(f.updatedAt))}</td>
            <td class="px-3 py-2">
              <div class="flex justify-end gap-1.5">
                <button type="button" class="tcp-btn px-2.5" data-use-chat="${idx}" title="Use in chat">Use in Chat</button>
                <a class="tcp-btn px-2.5" href="/api/files/download?path=${encodeURIComponent(f.path)}" title="Download"><i data-lucide="download"></i></a>
                <button type="button" class="tcp-btn-danger px-2.5" data-del-file="${idx}" title="Delete"><i data-lucide="trash-2"></i></button>
              </div>
            </td>
          </tr>
        `
      )
      .join("");

    tableEl.querySelectorAll("[data-use-chat]").forEach((el) => {
      el.addEventListener("click", () => {
        const i = Number(el.dataset.useChat);
        if (!Number.isFinite(i) || !files[i]) return;
        const pick = files[i];
        const exists = (state.chatUploadedFiles || []).some((x) => x.path === pick.path);
        if (!exists) {
          state.chatUploadedFiles = [
            {
              name: pick.name,
              path: pick.path,
              size: pick.size,
              mime: pick.mime || inferMime(pick.name),
              url: pick.url || pick.path,
              attach: true,
            },
            ...(state.chatUploadedFiles || []),
          ];
        }
        toast("ok", `${pick.name} added to chat attachments.`);
        setRoute("chat");
      });
    });

    tableEl.querySelectorAll("[data-del-file]").forEach((el) => {
      el.addEventListener("click", async () => {
        const i = Number(el.dataset.delFile);
        if (!Number.isFinite(i) || !files[i]) return;
        const pick = files[i];
        if (!window.confirm(`Delete ${pick.path}?`)) return;
        try {
          await api("/api/files/delete", { method: "POST", body: JSON.stringify({ path: pick.path }) });
          toast("ok", `${pick.name} deleted.`);
          await refreshFiles();
        } catch (e) {
          toast("err", e instanceof Error ? e.message : String(e));
        }
      });
    });

    renderIcons(tableEl);
  }

  async function fetchFilesFromEngine(dir) {
    const enginePath = dir ? `${ENGINE_UPLOADS_PREFIX}/${dir}` : ENGINE_UPLOADS_PREFIX;
    const qs = `?path=${encodeURIComponent(enginePath)}`;
    const raw = await api(`/api/engine/global/storage/files${qs}`);
    return normalizeFilesPayload(raw);
  }

  async function fetchFilesFallback(dir) {
    const qs = dir ? `?dir=${encodeURIComponent(dir)}` : "";
    const raw = await api(`/api/files/list${qs}`);
    return normalizeFilesPayload(raw);
  }

  async function refreshFiles() {
    try {
      let rows = [];
      try {
        rows = await fetchFilesFromEngine(currentDir);
      } catch {
        rows = await fetchFilesFallback(currentDir);
      }
      const out = rows
        .map((row) => {
          const p = normalizeDir(row.path || "");
          if (!p) return null;
          const name = p.split("/").pop() || p;
          return { ...row, kind: "file", name, path: p };
        })
        .filter(Boolean)
        .sort((a, b) => a.path.localeCompare(b.path));
      files.splice(0, files.length, ...out);
      renderTable();
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  }

  function uploadOne(file) {
    return new Promise((resolve, reject) => {
      const id = `${Date.now()}-${Math.random().toString(16).slice(2)}`;
      uploadState.set(id, { name: file.name, progress: 0, error: "" });
      renderProgress();

      const xhr = new XMLHttpRequest();
      const qs = currentDir ? `?dir=${encodeURIComponent(currentDir)}` : "";
      xhr.open("POST", `/api/files/upload${qs}`);
      xhr.withCredentials = true;
      xhr.responseType = "json";
      xhr.setRequestHeader("x-file-name", encodeURIComponent(file.name));

      xhr.upload.onprogress = (ev) => {
        if (!ev.lengthComputable) return;
        const row = uploadState.get(id);
        if (!row) return;
        row.progress = (ev.loaded / ev.total) * 100;
        renderProgress();
      };

      xhr.onerror = () => {
        const row = uploadState.get(id);
        if (row) row.error = "Network error";
        renderProgress();
        setTimeout(() => {
          uploadState.delete(id);
          renderProgress();
        }, 1200);
        reject(new Error(`Upload failed: ${file.name}`));
      };

      xhr.onload = () => {
        const payload = xhr.response || {};
        if (xhr.status < 200 || xhr.status >= 300 || payload?.ok === false) {
          const message = payload?.error || `Upload failed (${xhr.status})`;
          const row = uploadState.get(id);
          if (row) row.error = String(message);
          renderProgress();
          setTimeout(() => {
            uploadState.delete(id);
            renderProgress();
          }, 1800);
          reject(new Error(String(message)));
          return;
        }
        uploadState.delete(id);
        renderProgress();
        resolve(payload);
      };

      xhr.send(file);
    });
  }

  async function uploadFiles(fileList) {
    const items = [...(fileList || [])];
    if (!items.length) return;
    let ok = 0;
    for (const file of items) {
      try {
        await uploadOne(file);
        ok += 1;
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    }
    if (ok > 0) {
      toast("ok", `Uploaded ${ok} file${ok === 1 ? "" : "s"} to tandem/${currentDir || "."}.`);
      await refreshFiles();
    }
  }

  byId("files-refresh").addEventListener("click", () => {
    void refreshFiles();
  });
  uploadInputEl.addEventListener("change", async () => {
    await uploadFiles(uploadInputEl.files);
    uploadInputEl.value = "";
  });

  renderIcons(byId("view"));
  await refreshFiles();
}
