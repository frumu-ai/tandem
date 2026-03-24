import { readFile, readdir, stat } from "fs/promises";
import { resolve } from "path";
import { deriveRunBudget, inferStatusFromEvents, mapOrchestratorPath } from "../services/orchestratorService.js";

const _metrics = {
  streams_active: 0,
  streams_total: 0,
  events_received: 0,
  stream_errors: 0,
};

export function getOrchestratorMetrics() {
  return { ..._metrics };
}

export function createSwarmApiHandler(deps) {
  const {
    PORTAL_PORT,
    REPO_ROOT,
    ENGINE_URL,
    swarmState,
    getSwarmRunController,
    upsertSwarmRunController,
    setActiveSwarmRunId,
    isLocalEngineUrl,
    sendJson,
    readJsonBody,
    workspaceExistsAsDirectory,
    loadHiddenSwarmRunIds,
    saveHiddenSwarmRunIds,
    engineRequestJson,
    appendContextRunEvent,
    contextRunStatusToSwarmStatus,
    startSwarm,
    detectExecutorMode,
    startRunExecutor,
    requeueInProgressSteps,
    transitionBlackboardTask,
    contextRunSnapshot,
    contextRunToTasks,
  } = deps;

  return async function handleSwarmApi(req, res, session) {
    const url = new URL(req.url, `http://127.0.0.1:${PORTAL_PORT}`);
    const routePath = mapOrchestratorPath(url.pathname);
    const resolveRunId = (...values) =>
      values
        .map((value) => String(value || "").trim())
        .find((value) => value.length > 0) || "";
    const controllerFor = (runId = "") => getSwarmRunController(resolveRunId(runId));
    const readRunSetting = (runId, key, fallback) => {
      const controller = controllerFor(runId);
      return controller?.[key] ?? fallback;
    };
    const statusFromRun = async (runId) => {
      if (!runId) return null;
      try {
        const payload = await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`);
        return payload?.run || null;
      } catch {
        return null;
      }
    };

    if (routePath === "/api/swarm/status" && req.method === "GET") {
      const statusRunId = resolveRunId(url.searchParams.get("runId"), swarmState.runId);
      const run = await statusFromRun(statusRunId);
      if (run) {
        let status = contextRunStatusToSwarmStatus(run.status);
        if (status === "planning") {
          const eventsPayload = await engineRequestJson(
            session,
            `/context/runs/${encodeURIComponent(String(run?.run_id || statusRunId || ""))}/events?tail=60`
          ).catch(() => ({ events: [] }));
          status = contextRunStatusToSwarmStatus(inferStatusFromEvents(status, eventsPayload?.events));
        }
        if (statusRunId) {
          upsertSwarmRunController(statusRunId, {
            status,
            objective: String(run.objective || ""),
            workspaceRoot: String(run.workspace?.canonical_path || swarmState.workspaceRoot || REPO_ROOT),
            repoRoot: String(run.workspace?.canonical_path || swarmState.workspaceRoot || REPO_ROOT),
          });
        }
      } else if (statusRunId) {
        upsertSwarmRunController(statusRunId, {
          status: "idle",
          stoppedAt: Date.now(),
        });
      }
      const controller = controllerFor(statusRunId);
      sendJson(res, 200, {
        ok: true,
        status: controller?.status || swarmState.status,
        objective: controller?.objective || swarmState.objective,
        workspaceRoot: controller?.workspaceRoot || swarmState.workspaceRoot,
        maxTasks: controller?.maxTasks ?? swarmState.maxTasks,
        maxAgents: controller?.maxAgents ?? swarmState.maxAgents,
        workflowId: controller?.workflowId || swarmState.workflowId || "",
        modelProvider: controller?.modelProvider || swarmState.modelProvider || "",
        modelId: controller?.modelId || swarmState.modelId || "",
        resolvedModelProvider: controller?.resolvedModelProvider || swarmState.resolvedModelProvider || "",
        resolvedModelId: controller?.resolvedModelId || swarmState.resolvedModelId || "",
        modelResolutionSource: controller?.modelResolutionSource || swarmState.modelResolutionSource || "none",
        mcpServers: Array.isArray(controller?.mcpServers) ? controller.mcpServers : Array.isArray(swarmState.mcpServers) ? swarmState.mcpServers : [],
        repoRoot: controller?.repoRoot || swarmState.repoRoot || "",
        preflight: swarmState.preflight || null,
        startedAt: controller?.startedAt ?? swarmState.startedAt,
        stoppedAt: controller?.stoppedAt ?? swarmState.stoppedAt,
        runId: statusRunId || swarmState.runId || "",
        attachedPid: controller?.attachedPid || swarmState.attachedPid || null,
        localEngine: isLocalEngineUrl(ENGINE_URL),
        lastError: controller?.lastError || swarmState.lastError || null,
        executorState: controller?.executorState || swarmState.executorState || "idle",
        executorReason: controller?.executorReason || swarmState.executorReason || null,
        executorMode: controller?.executorMode || swarmState.executorMode || "context_steps",
        verificationMode: controller?.verificationMode || swarmState.verificationMode || "strict",
        currentRunId: statusRunId || swarmState.runId || "",
        buildVersion: swarmState.buildVersion || "",
        buildFingerprint: swarmState.buildFingerprint || "",
        buildStartedAt: swarmState.buildStartedAt || null,
      });
      return true;
    }

    if (routePath === "/api/swarm/runs" && req.method === "GET") {
      const workspace = String(url.searchParams.get("workspace") || "").trim();
      const query = workspace ? `?workspace=${encodeURIComponent(resolve(workspace))}&limit=100` : "?limit=100";
      const payload = await engineRequestJson(session, `/context/runs${query}`).catch(() => ({ runs: [] }));
      const includeHidden = String(url.searchParams.get("include_hidden") || "").trim() === "1";
      const hiddenRunIds = await loadHiddenSwarmRunIds();
      const allRuns = Array.isArray(payload?.runs) ? payload.runs : [];
      const runs = includeHidden
        ? allRuns
        : allRuns.filter((run) => !hiddenRunIds.has(String(run?.run_id || "").trim()));
      const active = runs.filter((run) => {
        const status = String(run?.status || "").toLowerCase();
        return !["completed", "failed", "cancelled"].includes(status);
      });
      sendJson(res, 200, {
        ok: true,
        runs,
        active,
        recent: runs.slice(0, 30),
        hiddenCount: hiddenRunIds.size,
      });
      return true;
    }

    if (routePath === "/api/swarm/workspaces/list" && req.method === "GET") {
      try {
        const requestedDir = String(url.searchParams.get("dir") || swarmState.workspaceRoot || REPO_ROOT).trim();
        const currentDir = await workspaceExistsAsDirectory(requestedDir);
        if (!currentDir) throw new Error(`Directory not found: ${resolve(requestedDir || REPO_ROOT)}`);
        const entries = await readdir(currentDir, { withFileTypes: true });
        const directories = entries
          .filter((entry) => entry.isDirectory())
          .map((entry) => ({
            name: entry.name,
            path: resolve(currentDir, entry.name),
          }))
          .sort((a, b) => a.name.localeCompare(b.name))
          .slice(0, 500);
        const parent = resolve(currentDir, "..");
        sendJson(res, 200, {
          ok: true,
          dir: currentDir,
          parent: parent === currentDir ? null : parent,
          directories,
        });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/workspaces/files" && req.method === "GET") {
      try {
        const workspaceRootRaw = String(
          url.searchParams.get("workspaceRoot") || swarmState.workspaceRoot || REPO_ROOT
        ).trim();
        const workspaceRoot = await workspaceExistsAsDirectory(workspaceRootRaw);
        if (!workspaceRoot) throw new Error(`Workspace not found: ${resolve(workspaceRootRaw || REPO_ROOT)}`);
        const requestedDir = String(url.searchParams.get("dir") || workspaceRoot).trim();
        const currentDir = await workspaceExistsAsDirectory(requestedDir);
        if (!currentDir) throw new Error(`Directory not found: ${resolve(requestedDir || workspaceRoot)}`);
        if (currentDir !== workspaceRoot && !currentDir.startsWith(`${workspaceRoot}/`)) {
          throw new Error("Directory must be inside workspace root.");
        }
        const entries = await readdir(currentDir, { withFileTypes: true });
        const directories = entries
          .filter((entry) => entry.isDirectory())
          .map((entry) => ({
            name: entry.name,
            path: resolve(currentDir, entry.name),
          }))
          .sort((a, b) => a.name.localeCompare(b.name))
          .slice(0, 300);
        const files = (
          await Promise.all(
            entries
              .filter((entry) => entry.isFile())
              .slice(0, 500)
              .map(async (entry) => {
                const path = resolve(currentDir, entry.name);
                const info = await stat(path).catch(() => null);
                if (!info || !info.isFile()) return null;
                return {
                  name: entry.name,
                  path,
                  size: Number(info.size || 0),
                  updatedAt: Number(info.mtimeMs || 0),
                };
              })
          )
        )
          .filter(Boolean)
          .sort((a, b) => Number(b.updatedAt || 0) - Number(a.updatedAt || 0));
        const parent = resolve(currentDir, "..");
        const insideParent =
          parent === workspaceRoot || (parent !== currentDir && parent.startsWith(`${workspaceRoot}/`));
        sendJson(res, 200, {
          ok: true,
          workspaceRoot,
          dir: currentDir,
          parent: insideParent ? parent : null,
          directories,
          files,
        });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/workspaces/read" && req.method === "GET") {
      try {
        const workspaceRootRaw = String(
          url.searchParams.get("workspaceRoot") || swarmState.workspaceRoot || REPO_ROOT
        ).trim();
        const workspaceRoot = await workspaceExistsAsDirectory(workspaceRootRaw);
        if (!workspaceRoot) throw new Error(`Workspace not found: ${resolve(workspaceRootRaw || REPO_ROOT)}`);
        const filePath = resolve(String(url.searchParams.get("path") || "").trim());
        if (!filePath) throw new Error("Missing file path.");
        if (filePath !== workspaceRoot && !filePath.startsWith(`${workspaceRoot}/`)) {
          throw new Error("File must be inside workspace root.");
        }
        const info = await stat(filePath);
        if (!info.isFile()) throw new Error("Not a file.");
        if (info.size > 1024 * 1024) throw new Error("File is too large to preview.");
        const text = await readFile(filePath, "utf8");
        sendJson(res, 200, {
          ok: true,
          workspaceRoot,
          path: filePath,
          size: Number(info.size || 0),
          text,
        });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/runs/hide" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runIds = (Array.isArray(body?.runIds) ? body.runIds : [])
          .map((id) => String(id || "").trim())
          .filter(Boolean)
          .slice(0, 500);
        if (!runIds.length) throw new Error("Missing runIds");
        const hidden = await loadHiddenSwarmRunIds();
        for (const runId of runIds) hidden.add(runId);
        await saveHiddenSwarmRunIds(hidden);
        if (runIds.includes(String(swarmState.runId || "").trim())) {
          setActiveSwarmRunId("");
        }
        sendJson(res, 200, { ok: true, hiddenCount: hidden.size, hiddenRunIds: runIds });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/runs/unhide" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runIds = (Array.isArray(body?.runIds) ? body.runIds : [])
          .map((id) => String(id || "").trim())
          .filter(Boolean)
          .slice(0, 500);
        if (!runIds.length) throw new Error("Missing runIds");
        const hidden = await loadHiddenSwarmRunIds();
        for (const runId of runIds) hidden.delete(runId);
        await saveHiddenSwarmRunIds(hidden);
        sendJson(res, 200, { ok: true, hiddenCount: hidden.size, unhiddenRunIds: runIds });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/runs/hide_completed" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const workspace = String(body?.workspace || "").trim();
        const query = workspace ? `?workspace=${encodeURIComponent(resolve(workspace))}&limit=1000` : "?limit=1000";
        const payload = await engineRequestJson(session, `/context/runs${query}`).catch(() => ({ runs: [] }));
        const allRuns = Array.isArray(payload?.runs) ? payload.runs : [];
        const completedRunIds = allRuns
          .filter((run) => {
            const status = String(run?.status || "").toLowerCase();
            return ["completed", "failed", "cancelled"].includes(status);
          })
          .map((run) => String(run?.run_id || "").trim())
          .filter(Boolean);
        const hidden = await loadHiddenSwarmRunIds();
        for (const runId of completedRunIds) hidden.add(runId);
        await saveHiddenSwarmRunIds(hidden);
        if (completedRunIds.includes(String(swarmState.runId || "").trim())) {
          setActiveSwarmRunId("");
        }
        sendJson(res, 200, { ok: true, hiddenCount: hidden.size, hiddenNow: completedRunIds.length });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/start" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = await startSwarm(session, body || {});
        sendJson(res, 200, { ok: true, runId });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/request_revision" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = resolveRunId(body?.runId, swarmState.runId);
        const controller = controllerFor(runId);
        const feedback = String(body?.feedback || "").trim();
        if (!runId) throw new Error("Missing runId");
        if (!feedback) throw new Error("Missing revision feedback");
        const payload = await engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}`);
        const run = payload?.run || {};
        const objective = String(run?.objective || "").trim();
        const workspaceRoot = String(
          run?.workspace?.canonical_path ||
            run?.workspace_root ||
            controller?.workspaceRoot ||
            swarmState.workspaceRoot ||
            ""
        ).trim();
        if (!objective || !workspaceRoot) {
          throw new Error("Cannot request revision: missing objective/workspace from existing run.");
        }
        await appendContextRunEvent(session, runId, "revision_requested", "planning", {
          feedback,
        }).catch(() => null);
        const revisedObjective = `${objective}\n\nRevision feedback:\n${feedback}`;
        const revisedRunId = await startSwarm(session, {
          workspaceRoot,
          objective: revisedObjective,
          maxTasks: Number(body?.maxTasks || controller?.maxTasks || swarmState.maxTasks || 3),
          maxAgents: Number(body?.maxAgents || controller?.maxAgents || swarmState.maxAgents || 3),
          workflowId: String(
            body?.workflowId ||
              controller?.workflowId ||
              swarmState.workflowId ||
              "swarm.blackboard.default"
          ),
          modelProvider: String(
            run?.model_provider || controller?.modelProvider || swarmState.modelProvider || ""
          ),
          modelId: String(run?.model_id || controller?.modelId || swarmState.modelId || ""),
          mcpServers: Array.isArray(controller?.mcpServers)
            ? controller.mcpServers
            : Array.isArray(swarmState.mcpServers)
              ? swarmState.mcpServers
              : [],
          verificationMode: String(
            body?.verificationMode ||
              controller?.verificationMode ||
              swarmState.verificationMode ||
              "strict"
          ),
          allowLocalPlannerFallback: body?.allowLocalPlannerFallback === true,
        });
        sendJson(res, 200, { ok: true, runId: revisedRunId, previousRunId: runId });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/approve" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = resolveRunId(body?.runId, swarmState.runId);
        const controller = controllerFor(runId);
        if (!runId) throw new Error("Missing runId");
        setActiveSwarmRunId(runId);
        await appendContextRunEvent(session, runId, "plan_approved", "running", {});
        const mode = await detectExecutorMode(session, runId);
        void startRunExecutor(session, runId, {
          mode,
          maxAgents: controller?.maxAgents ?? swarmState.maxAgents,
          workflowId: controller?.workflowId || swarmState.workflowId,
        });
        sendJson(res, 200, { ok: true, runId });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/pause" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = resolveRunId(body?.runId, swarmState.runId);
        if (!runId) throw new Error("Missing runId");
        setActiveSwarmRunId(runId);
        await appendContextRunEvent(session, runId, "run_paused", "paused", {});
        sendJson(res, 200, { ok: true, runId });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/resume" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = resolveRunId(body?.runId, swarmState.runId);
        const controller = controllerFor(runId);
        if (!runId) throw new Error("Missing runId");
        setActiveSwarmRunId(runId);
        await appendContextRunEvent(session, runId, "run_resumed", "running", {});
        const requeued = await requeueInProgressSteps(session, runId);
        const mode = await detectExecutorMode(session, runId);
        const started = await startRunExecutor(session, runId, {
          mode,
          maxAgents: controller?.maxAgents ?? swarmState.maxAgents,
          workflowId: controller?.workflowId || swarmState.workflowId,
        });
        const preview = await engineRequestJson(
          session,
          `/context/runs/${encodeURIComponent(runId)}/driver/next`,
          { method: "POST", body: { dry_run: true } }
        ).catch(() => null);
        sendJson(res, 200, {
          ok: true,
          runId,
          started,
          requeued,
          sessionDispatchOutcome: started ? "started" : "already_running",
          selectedStepId: preview?.selected_step_id || null,
          whyNextStep: preview?.why_next_step || null,
          executorMode: readRunSetting(runId, "executorMode", mode),
          executorState: readRunSetting(runId, "executorState", "idle"),
          executorReason: readRunSetting(runId, "executorReason", null),
        });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/continue" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = resolveRunId(body?.runId, swarmState.runId);
        const controller = controllerFor(runId);
        if (!runId) throw new Error("Missing runId");
        setActiveSwarmRunId(runId);
        await appendContextRunEvent(session, runId, "run_resumed", "running", {
          why_next_step: "manual continue requested",
        });
        const requeued = await requeueInProgressSteps(session, runId);
        const mode = await detectExecutorMode(session, runId);
        const started = await startRunExecutor(session, runId, {
          mode,
          maxAgents: controller?.maxAgents ?? swarmState.maxAgents,
          workflowId: controller?.workflowId || swarmState.workflowId,
        });
        const preview = await engineRequestJson(
          session,
          `/context/runs/${encodeURIComponent(runId)}/driver/next`,
          { method: "POST", body: { dry_run: true } }
        ).catch(() => null);
        sendJson(res, 200, {
          ok: true,
          runId,
          started,
          requeued,
          sessionDispatchOutcome: started ? "started" : "already_running",
          selectedStepId: preview?.selected_step_id || null,
          whyNextStep: preview?.why_next_step || null,
          executorMode: readRunSetting(runId, "executorMode", mode),
          executorState: readRunSetting(runId, "executorState", "idle"),
          executorReason: readRunSetting(runId, "executorReason", null),
        });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if ((routePath === "/api/swarm/cancel" || routePath === "/api/swarm/stop") && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = resolveRunId(body?.runId, swarmState.runId);
        if (!runId) throw new Error("Missing runId");
        setActiveSwarmRunId(runId);
        await appendContextRunEvent(session, runId, "run_cancelled", "cancelled", {});
        if (swarmState.runId === runId) {
          upsertSwarmRunController(runId, {
            status: "cancelled",
            stoppedAt: Date.now(),
          });
        }
        sendJson(res, 200, { ok: true, runId });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/retry" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = resolveRunId(body?.runId, swarmState.runId);
        const stepId = String(body?.stepId || "").trim();
        if (!runId || !stepId) throw new Error("Missing runId or stepId");
        const controller = controllerFor(runId);
        setActiveSwarmRunId(runId);
        await transitionBlackboardTask(session, runId, { id: stepId }, { action: "retry" }).catch(
          () => null
        );
        await appendContextRunEvent(session, runId, "task_retry_requested", "running", {
          why_next_step: `manual retry requested for ${stepId}`,
        }, stepId);
        const mode = await detectExecutorMode(session, runId);
        const started = await startRunExecutor(session, runId, {
          mode,
          maxAgents: controller?.maxAgents ?? swarmState.maxAgents,
          workflowId: controller?.workflowId || swarmState.workflowId,
        });
        sendJson(res, 200, {
          ok: true,
          runId,
          stepId,
          started,
          sessionDispatchOutcome: started ? "started" : "already_running",
          executorMode: readRunSetting(runId, "executorMode", mode),
          executorState: readRunSetting(runId, "executorState", "idle"),
          executorReason: readRunSetting(runId, "executorReason", null),
        });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/tasks/create" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = resolveRunId(body?.runId, swarmState.runId);
        const tasks = Array.isArray(body?.tasks) ? body.tasks : [];
        if (!runId || !tasks.length) throw new Error("Missing runId or tasks");
        const payload = await engineRequestJson(
          session,
          `/context/runs/${encodeURIComponent(runId)}/tasks`,
          {
            method: "POST",
            body: { tasks },
          }
        );
        sendJson(res, 200, payload);
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/tasks/claim" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = resolveRunId(body?.runId, swarmState.runId);
        if (!runId) throw new Error("Missing runId");
        const controller = controllerFor(runId);
        const claimBody = {
          agent_id: String(body?.agentId || "control_panel").trim(),
          command_id: body?.commandId || undefined,
          task_type: body?.taskType || undefined,
          workflow_id: body?.workflowId || controller?.workflowId || undefined,
          lease_ms: Number(body?.leaseMs || 30000),
        };
        const payload = await engineRequestJson(
          session,
          `/context/runs/${encodeURIComponent(runId)}/tasks/claim`,
          {
            method: "POST",
            body: claimBody,
          }
        );
        sendJson(res, 200, payload);
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/tasks/transition" && req.method === "POST") {
      try {
        const body = await readJsonBody(req);
        const runId = resolveRunId(body?.runId, swarmState.runId);
        const taskId = String(body?.taskId || "").trim();
        if (!runId || !taskId) throw new Error("Missing runId or taskId");
        const transitionBody = {
          action: body?.action || "status",
          command_id: body?.commandId || undefined,
          expected_task_rev: body?.expectedTaskRev ?? undefined,
          lease_token: body?.leaseToken || undefined,
          agent_id: body?.agentId || undefined,
          status: body?.status || undefined,
          error: body?.error || undefined,
          lease_ms: body?.leaseMs || undefined,
        };
        const payload = await engineRequestJson(
          session,
          `/context/runs/${encodeURIComponent(runId)}/tasks/${encodeURIComponent(taskId)}/transition`,
          {
            method: "POST",
            body: transitionBody,
          }
        );
        sendJson(res, 200, payload);
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath.startsWith("/api/swarm/run/") && req.method === "GET") {
      const runId = decodeURIComponent(routePath.replace("/api/swarm/run/", "").trim());
      if (!runId) {
        sendJson(res, 400, { ok: false, error: "Missing run id." });
        return true;
      }
      try {
        const snapshot = await contextRunSnapshot(session, runId);
        const boardTasks = Array.isArray(snapshot?.run?.tasks)
          ? snapshot.run.tasks
          : Array.isArray(snapshot?.blackboard?.tasks)
            ? snapshot.blackboard.tasks
            : [];
        const controller = controllerFor(runId);
        if (boardTasks.length) {
          const workflow = String(boardTasks[0]?.workflow_id || "").trim();
          upsertSwarmRunController(runId, {
            workflowId: workflow || controller?.workflowId || swarmState.workflowId,
            executorMode: "blackboard",
          });
        } else {
          upsertSwarmRunController(runId, { executorMode: "context_steps" });
        }
        const effectiveRunStatus = contextRunStatusToSwarmStatus(
          inferStatusFromEvents(snapshot.run?.status, snapshot.events)
        );
        upsertSwarmRunController(runId, {
          status: effectiveRunStatus,
          objective: String(snapshot.run?.objective || controller?.objective || ""),
          workspaceRoot: String(
            snapshot.run?.workspace?.canonical_path ||
              controller?.workspaceRoot ||
              swarmState.workspaceRoot ||
              ""
          ),
          repoRoot: String(
            snapshot.run?.workspace?.canonical_path ||
              controller?.repoRoot ||
              swarmState.workspaceRoot ||
              ""
          ),
          stoppedAt: ["completed", "failed", "cancelled"].includes(effectiveRunStatus)
            ? Number(snapshot.run?.updated_at_ms || Date.now())
            : null,
        });
        sendJson(res, 200, {
          ok: true,
          run: snapshot.run,
          runStatus: effectiveRunStatus,
          events: snapshot.events,
          blackboard: snapshot.blackboard,
          blackboardPatches: snapshot.blackboardPatches,
          replay: snapshot.replay,
          budget: deriveRunBudget(snapshot.run, snapshot.events, boardTasks),
          tasks: contextRunToTasks(snapshot.run),
          controller: getSwarmRunController(runId),
        });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/snapshot" && req.method === "GET") {
      const runId = resolveRunId(url.searchParams.get("runId"), swarmState.runId);
      if (!runId) {
        sendJson(res, 200, {
          ok: true,
          status: "idle",
          registry: { key: "context.run.steps", value: { version: 1, updatedAtMs: Date.now(), tasks: {} } },
          reasons: [],
          logs: [],
          startedAt: swarmState.startedAt,
          stoppedAt: swarmState.stoppedAt,
          lastError: swarmState.lastError || null,
        });
        return true;
      }
      try {
        const snapshot = await contextRunSnapshot(session, runId);
        const effectiveStatus = contextRunStatusToSwarmStatus(snapshot.run?.status);
        upsertSwarmRunController(runId, {
          registryCache: snapshot.registry,
          logs: snapshot.logs,
          reasons: snapshot.reasons,
          status: effectiveStatus,
          objective: String(snapshot.run?.objective || ""),
          workspaceRoot: String(
            snapshot.run?.workspace?.canonical_path || swarmState.workspaceRoot || ""
          ),
          repoRoot: String(snapshot.run?.workspace?.canonical_path || swarmState.workspaceRoot || ""),
        });
        sendJson(res, 200, {
          ok: true,
          status: effectiveStatus,
          registry: snapshot.registry,
          reasons: snapshot.reasons,
          logs: snapshot.logs,
          run: snapshot.run,
          startedAt: Number(
            snapshot.run?.started_at_ms ||
              readRunSetting(runId, "startedAt", swarmState.startedAt) ||
              Date.now()
          ),
          stoppedAt: isRunTerminalStatus(snapshot.run?.status)
            ? Number(snapshot.run?.updated_at_ms || Date.now())
            : null,
          lastError: readRunSetting(runId, "lastError", swarmState.lastError) || null,
        });
      } catch (e) {
        sendJson(res, 400, { ok: false, error: e instanceof Error ? e.message : String(e) });
      }
      return true;
    }

    if (routePath === "/api/swarm/events/health" && req.method === "GET") {
      const requestedWorkspace = String(url.searchParams.get("workspace") || "").trim();
      const workspace = String(requestedWorkspace || swarmState.workspaceRoot || REPO_ROOT).trim();
      const runIds = String(url.searchParams.get("runIds") || "")
        .split(",")
        .map((row) => String(row || "").trim())
        .filter(Boolean);
      const query = new URLSearchParams();
      if (workspace) query.set("workspace", workspace);
      if (runIds.length) query.set("run_ids", runIds.join(","));
      query.set("tail", "1");
      const engineProbeUrl = `${ENGINE_URL}/context/runs/events/stream?${query.toString()}`;
      let multiplexAvailable = false;
      let multiplexStatus = 0;
      let multiplexError = "";
      let fallbackRunId = String(
        url.searchParams.get("runId") || runIds[0] || swarmState.runId || ""
      ).trim();

      try {
        const response = await fetch(engineProbeUrl, {
          method: "GET",
          headers: {
            accept: "text/event-stream",
            authorization: `Bearer ${session.token}`,
            "x-tandem-token": session.token,
          },
        });
        multiplexStatus = Number(response.status || 0);
        multiplexAvailable = response.ok;
        if (!response.ok) {
          multiplexError = `engine returned ${response.status}`;
        }
        response.body?.cancel?.().catch?.(() => null);
      } catch (error) {
        multiplexError = String(error?.message || error || "probe failed");
      }

      const fallbackAvailable = !!fallbackRunId;
      if (!fallbackRunId) {
        const statusRunId = String(swarmState.runId || "").trim();
        if (statusRunId) fallbackRunId = statusRunId;
      }

      sendJson(res, 200, {
        ok: true,
        mode: multiplexAvailable ? "multiplex" : "fallback",
        workspace: workspace || null,
        runIds,
        engineUrl: ENGINE_URL,
        engineProbeUrl,
        multiplex: {
          available: multiplexAvailable,
          status: multiplexStatus || null,
          error: multiplexError || null,
        },
        fallback: {
          available: fallbackAvailable,
          runId: fallbackRunId || null,
          endpoint: fallbackRunId ? `/api/orchestrator/events?runId=${encodeURIComponent(fallbackRunId)}` : null,
        },
      });
      return true;
    }

    if (routePath === "/api/swarm/events" && req.method === "GET") {
      const requestedWorkspace = String(url.searchParams.get("workspace") || "").trim();
      const workspace = String(requestedWorkspace || swarmState.workspaceRoot || REPO_ROOT).trim();
      const runIds = String(url.searchParams.get("runIds") || "")
        .split(",")
        .map((row) => String(row || "").trim())
        .filter(Boolean);
      const runId = String(
        url.searchParams.get("runId") || runIds[0] || swarmState.runId || ""
      ).trim();
      const cursor = String(url.searchParams.get("cursor") || "").trim();
      const tail = String(url.searchParams.get("tail") || "").trim();

      if (workspace) {
        const query = new URLSearchParams();
        query.set("workspace", workspace);
        const scopedRunIds = runIds.length
          ? runIds
          : runId
            ? [runId]
            : [];
        if (scopedRunIds.length) query.set("run_ids", scopedRunIds.join(","));
        if (cursor) query.set("cursor", cursor);
        if (tail) query.set("tail", tail);
        const targetUrl = `${ENGINE_URL}/context/runs/events/stream?${query.toString()}`;
        try {
          const upstream = await fetch(targetUrl, {
            method: "GET",
            headers: {
              accept: "text/event-stream",
              authorization: `Bearer ${session.token}`,
              "x-tandem-token": session.token,
            },
          });
          if (upstream.ok && upstream.body) {
            _metrics.streams_active += 1;
            _metrics.streams_total += 1;
            res.writeHead(200, {
              "content-type": "text/event-stream",
              "cache-control": "no-cache",
              connection: "keep-alive",
            });
            req.on("close", () => { _metrics.streams_active = Math.max(0, _metrics.streams_active - 1); });
            for await (const chunk of upstream.body) {
              if (res.writableEnded || res.destroyed) break;
              res.write(chunk);
              _metrics.events_received += 1;
            }
            if (!res.writableEnded && !res.destroyed) res.end();
            _metrics.streams_active = Math.max(0, _metrics.streams_active - 1);
            return true;
          }
        } catch {
          _metrics.stream_errors += 1;
          // fall back to legacy single-run poll bridge below
        }
      }

      res.writeHead(200, {
        "content-type": "text/event-stream",
        "cache-control": "no-cache",
        connection: "keep-alive",
      });
      let closed = false;
      let sinceSeq = 0;
      let sincePatchSeq = 0;
      const close = () => {
        closed = true;
      };
      req.on("close", close);
      res.write(
        `data: ${JSON.stringify({
          kind: "hello",
          ts: Date.now(),
          status: readRunSetting(runId, "status", swarmState.status),
          runId,
        })}\n\n`
      );
      const tick = async () => {
        if (closed || !runId) return;
        try {
          const [eventsPayload, patchesPayload] = await Promise.all([
            engineRequestJson(session, `/context/runs/${encodeURIComponent(runId)}/events?since_seq=${sinceSeq}`),
            engineRequestJson(
              session,
              `/context/runs/${encodeURIComponent(runId)}/blackboard/patches?since_seq=${sincePatchSeq}`
            ).catch(() => ({ patches: [] })),
          ]);
          const events = Array.isArray(eventsPayload?.events) ? eventsPayload.events : [];
          for (const event of events) {
            sinceSeq = Math.max(sinceSeq, Number(event?.seq || 0));
            res.write(
              `data: ${JSON.stringify({
                kind: "context_run_event",
                run_id: runId,
                seq: Number(event?.seq || 0),
                ts_ms: Date.now(),
                payload: event,
              })}\n\n`
            );
          }
          const patches = Array.isArray(patchesPayload?.patches) ? patchesPayload.patches : [];
          for (const patch of patches) {
            sincePatchSeq = Math.max(sincePatchSeq, Number(patch?.seq || 0));
            res.write(
              `data: ${JSON.stringify({
                kind: "blackboard_patch",
                run_id: runId,
                seq: Number(patch?.seq || 0),
                ts_ms: Date.now(),
                payload: patch,
              })}\n\n`
            );
          }
        } catch {
          _metrics.stream_errors += 1;
          // ignore transient poll failures
        }
      };
      const interval = setInterval(tick, 1500);
      tick();
      req.on("close", () => clearInterval(interval));
      return true;
    }

    sendJson(res, 404, { ok: false, error: "Unknown swarm route." });
    return true;
  };
}

function isRunTerminalStatus(status) {
  const normalized = String(status || "")
    .trim()
    .toLowerCase();
  return ["completed", "failed", "cancelled"].includes(normalized);
}
