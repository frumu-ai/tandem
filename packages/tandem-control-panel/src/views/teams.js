function toList(value) {
  return Array.isArray(value) ? value : [];
}

function idOf(record) {
  return record?.id || record?.instanceID || record?.missionID || record?.templateID || "(unknown)";
}

function statusOf(record) {
  return record?.status || record?.state || record?.phase || "unknown";
}

function matchFilter(record, term) {
  if (!term) return true;
  const hay = JSON.stringify(record || {}).toLowerCase();
  return hay.includes(term);
}

function renderRecordCards(rows, emptyText, escapeHtml, titleKey = "id") {
  if (!rows.length) return `<p class="tcp-subtle">${emptyText}</p>`;
  return rows
    .map((row) => {
      const mainId = escapeHtml(row?.[titleKey] || idOf(row));
      const status = escapeHtml(statusOf(row));
      return `
        <article class="tcp-list-item">
          <div class="flex items-center justify-between gap-2">
            <strong>${mainId}</strong>
            <span class="tcp-badge-info">${status}</span>
          </div>
          <div class="tcp-subtle mt-1">role: ${escapeHtml(row?.role || row?.ownerRole || "n/a")}</div>
          <div class="tcp-subtle">mission: ${escapeHtml(row?.missionID || row?.missionId || row?.mission || "n/a")}</div>
          <details class="mt-2">
            <summary class="cursor-pointer text-xs text-slate-400">Details</summary>
            <pre class="tcp-code mt-2">${escapeHtml(JSON.stringify(row, null, 2))}</pre>
          </details>
        </article>
      `;
    })
    .join("");
}

export async function renderTeams(ctx) {
  const { state, byId, toast, escapeHtml } = ctx;
  const [templatesRaw, instancesRaw, missionsRaw, approvalsRaw] = await Promise.all([
    state.client.agentTeams.listTemplates().catch(() => ({ templates: [] })),
    state.client.agentTeams.listInstances().catch(() => ({ instances: [] })),
    state.client.agentTeams.listMissions().catch(() => ({ missions: [] })),
    state.client.agentTeams.listApprovals().catch(() => ({ spawnApprovals: [] })),
  ]);

  const templates = toList(templatesRaw.templates);
  const instances = toList(instancesRaw.instances);
  const missions = toList(missionsRaw.missions);
  const approvals = toList(approvalsRaw.spawnApprovals);

  byId("view").innerHTML = `
    <div class="tcp-card">
      <h3 class="tcp-title mb-3">Spawn Agent Team Instance</h3>
      <div class="grid gap-3 md:grid-cols-4">
        <input id="team-mission" class="tcp-input" placeholder="missionID" />
        <input id="team-role" class="tcp-input" placeholder="role" value="worker" />
        <input id="team-template" class="tcp-input" placeholder="templateID" value="worker-default" />
        <button id="team-spawn" class="tcp-btn-primary">Spawn</button>
      </div>
    </div>

    <div class="tcp-card">
      <div class="mb-3 flex items-center justify-between gap-3">
        <h3 class="tcp-title">Teams & Missions</h3>
        <input id="teams-filter" class="tcp-input max-w-sm" placeholder="Filter instances/missions/templates" />
      </div>

      <div class="grid gap-4 lg:grid-cols-2">
        <section>
          <h4 class="mb-2 font-medium">Approvals (${approvals.length})</h4>
          <div id="team-approvals" class="tcp-list"></div>
        </section>
        <section>
          <h4 class="mb-2 font-medium">Instances (${instances.length})</h4>
          <div id="team-instances" class="tcp-list"></div>
        </section>
      </div>

      <div class="mt-4 grid gap-4 lg:grid-cols-2">
        <section>
          <h4 class="mb-2 font-medium">Missions (${missions.length})</h4>
          <div id="team-missions" class="tcp-list"></div>
        </section>
        <section>
          <h4 class="mb-2 font-medium">Templates (${templates.length})</h4>
          <div id="team-templates" class="tcp-list"></div>
        </section>
      </div>
    </div>
  `;

  byId("team-spawn").addEventListener("click", async () => {
    try {
      await state.client.agentTeams.spawn({
        missionID: byId("team-mission").value.trim(),
        role: byId("team-role").value.trim() || "worker",
        templateID: byId("team-template").value.trim() || "worker-default",
        source: "ui_action",
        justification: "spawn from control panel",
      });
      toast("ok", "Spawn requested.");
      renderTeams(ctx);
    } catch (e) {
      toast("err", e instanceof Error ? e.message : String(e));
    }
  });

  function renderFiltered() {
    const term = byId("teams-filter").value.trim().toLowerCase();

    const filteredInstances = instances.filter((rec) => matchFilter(rec, term));
    const filteredMissions = missions.filter((rec) => matchFilter(rec, term));
    const filteredTemplates = templates.filter((rec) => matchFilter(rec, term));

    byId("team-instances").innerHTML = renderRecordCards(filteredInstances, "No instances.", escapeHtml, "instanceID");
    byId("team-missions").innerHTML = renderRecordCards(filteredMissions, "No missions.", escapeHtml, "missionID");
    byId("team-templates").innerHTML = renderRecordCards(filteredTemplates, "No templates.", escapeHtml, "templateID");
  }

  const approvalList = byId("team-approvals");
  approvalList.innerHTML =
    approvals
      .map((a) => {
        const approvalID = escapeHtml(a.approvalID || a.id || "");
        return `
          <div class="tcp-list-item flex items-center justify-between gap-3">
            <div>
              <div><strong>${approvalID || "approval"}</strong></div>
              <div class="tcp-subtle">mission: ${escapeHtml(a.missionID || a.missionId || "n/a")}</div>
            </div>
            <div class="flex gap-2">
              <button data-ap="${approvalID}" class="tcp-btn-primary">Approve</button>
              <button data-den="${approvalID}" class="tcp-btn-danger">Deny</button>
            </div>
          </div>`;
      })
      .join("") || '<p class="tcp-subtle">No pending spawn approvals.</p>';

  approvalList.querySelectorAll("[data-ap]").forEach((b) =>
    b.addEventListener("click", async () => {
      try {
        await state.client.agentTeams.approveSpawn(b.dataset.ap, "approved in portal");
        toast("ok", "Approved.");
        renderTeams(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );

  approvalList.querySelectorAll("[data-den]").forEach((b) =>
    b.addEventListener("click", async () => {
      try {
        await state.client.agentTeams.denySpawn(b.dataset.den, "denied in portal");
        toast("ok", "Denied.");
        renderTeams(ctx);
      } catch (e) {
        toast("err", e instanceof Error ? e.message : String(e));
      }
    })
  );

  byId("teams-filter").addEventListener("input", renderFiltered);
  renderFiltered();
}
