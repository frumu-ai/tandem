import { useMutation, useQueries, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  applyEdgeChanges,
  applyNodeChanges,
  type Connection,
  type EdgeChange,
  type NodeChange,
} from "@xyflow/react";
import type {
  OrchestrationEdgeSpec,
  OrchestrationNodeSpec,
  OrchestrationSpec,
  OrchestrationSummary,
  OrchestrationValidationIssue,
} from "@frumu/tandem-client";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  OrchestrationCanvas,
  toCanvasEdges,
  toCanvasNodes,
  type OrchestrationSelection,
} from "../features/orchestration-studio/OrchestrationCanvas";
import { OrchestrationInspector } from "../features/orchestration-studio/OrchestrationInspector";
import { analyzeGraph } from "../features/orchestration-studio/graph";
import { autoLayoutLeftToRight } from "../features/orchestration-studio/layout";
import { validateOrchestrationDraft } from "../features/orchestration-studio/validation";
import {
  PaletteItem,
  automationId,
  automationName,
  compareOrchestrationSpecs,
  createOrchestrationNode,
  formatOrchestrationTime,
  orchestrationEditorId,
  orchestrationGraphSnapshot,
  sameStringSet,
  synchronizeWorkflowTransitionKeys,
} from "../features/orchestration-studio/pagePrimitives";
import { Icon } from "../ui/Icon";
import { SearchInput } from "../ui/index.tsx";
import { EmptyState, PageCard } from "./ui";
import type { AppPageProps } from "./pageTypes";

type LibraryFilter = "all" | "draft" | "published" | "archived" | "invalid" | "stale";
type EditorMode = "canvas" | "outline";

export function OrchestrationsPage({ client, toast, setNavigationLock, navigate }: AppPageProps) {
  const queryClient = useQueryClient();
  const [filter, setFilter] = useState<LibraryFilter>("all");
  const [search, setSearch] = useState("");
  const [selectedId, setSelectedId] = useState("");
  const selectedIdRef = useRef(selectedId);
  selectedIdRef.current = selectedId;
  const [draft, setDraft] = useState<OrchestrationSpec | null>(null);
  const [selection, setSelection] = useState<OrchestrationSelection>(null);
  const [selectedNodeIds, setSelectedNodeIds] = useState<Set<string>>(() => new Set());
  const [selectedEdgeIds, setSelectedEdgeIds] = useState<Set<string>>(() => new Set());
  const [editorMode, setEditorMode] = useState<EditorMode>("canvas");
  const [paletteSearch, setPaletteSearch] = useState("");
  const [dirty, setDirty] = useState(false);
  const [saveState, setSaveState] = useState<"saved" | "saving" | "conflict" | "error">("saved");
  const [showCreate, setShowCreate] = useState(false);
  const [newName, setNewName] = useState("");
  const [goalObjective, setGoalObjective] = useState("");
  const [previewFrom, setPreviewFrom] = useState("");
  const [previewKey, setPreviewKey] = useState("");
  const [previewResult, setPreviewResult] = useState<any>(null);
  const [transitionSource, setTransitionSource] = useState("");
  const [transitionTarget, setTransitionTarget] = useState("");
  const [transitionKey, setTransitionKey] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState(-1);
  const hydratedId = useRef("");
  const copiedNode = useRef<OrchestrationNodeSpec | null>(null);
  const revisionRef = useRef(0);
  const saveInFlightRef = useRef(false);

  const listQuery = useQuery({
    queryKey: ["orchestrations", "library"],
    queryFn: () => client.orchestrations.list({ limit: 100 }),
    refetchInterval: 15_000,
  });
  const automationsQuery = useQuery({
    queryKey: ["orchestrations", "automations"],
    queryFn: () => client.automationsV2.list(),
    refetchInterval: 30_000,
  });
  const goalsQuery = useQuery({
    queryKey: ["orchestrations", "goals"],
    queryFn: () => client.statefulRuntime.listGoals({ limit: 200 }),
    refetchInterval: 15_000,
  });
  const aggregateQuery = useQuery({
    queryKey: ["orchestrations", selectedId],
    enabled: !!selectedId,
    queryFn: () => client.orchestrations.get(selectedId),
  });
  const versionsQuery = useQuery({
    queryKey: ["orchestrations", selectedId, "versions"],
    enabled: !!selectedId,
    queryFn: () => client.orchestrations.listVersions(selectedId),
  });
  const validationQuery = useQuery({
    queryKey: ["orchestrations", selectedId, "validation"],
    enabled: !!selectedId && aggregateQuery.data?.draft?.status === "draft",
    queryFn: () => client.orchestrations.validate(selectedId),
    retry: false,
  });
  const staleQuery = useQuery({
    queryKey: ["orchestrations", selectedId, "stale"],
    enabled: !!selectedId && !!aggregateQuery.data?.draft,
    queryFn: () => client.orchestrations.staleReferences(selectedId),
    retry: false,
  });

  const summaries = listQuery.data?.orchestrations || [];
  const inspectLibraryHealth = filter === "invalid" || filter === "stale";
  const statusQueries = useQueries({
    queries: summaries.map((summary) => ({
      queryKey: [
        "orchestrations",
        summary.orchestration_id,
        "library-status",
        inspectLibraryHealth,
      ],
      queryFn: async () => {
        const aggregate = await client.orchestrations
          .get(summary.orchestration_id)
          .catch(() => null);
        const inspectDraft = aggregate?.draft?.status === "draft";
        const [validation, stale] = inspectLibraryHealth && inspectDraft
          ? await Promise.all([
              client.orchestrations.validate(summary.orchestration_id).catch(() => null),
              client.orchestrations.staleReferences(summary.orchestration_id).catch(() => null),
            ])
          : [null, null];
        return {
          spec: aggregate?.draft || aggregate?.latest_published || null,
          valid: validation?.report.valid,
          stale: stale?.references.filter((reference) => reference.state !== "fresh").length,
        };
      },
      staleTime: 60_000,
    })),
  });
  const statusById = useMemo(
    () =>
      new Map(
        summaries.map((summary, index) => [summary.orchestration_id, statusQueries[index]?.data])
      ),
    [statusQueries, summaries]
  );
  const publishedOnly = !!aggregateQuery.data?.latest_published && !aggregateQuery.data?.draft;
  const archivedDraft = aggregateQuery.data?.draft?.status === "archived";
  const readOnlySnapshot = publishedOnly || archivedDraft;

  useEffect(() => {
    const serverDraft = aggregateQuery.data?.draft || aggregateQuery.data?.latest_published;
    if (
      !serverDraft ||
      hydratedId.current === `${serverDraft.orchestration_id}:${serverDraft.updated_at_ms}`
    )
      return;
    if (dirty && hydratedId.current.startsWith(`${serverDraft.orchestration_id}:`)) return;
    setDraft(serverDraft);
    revisionRef.current = 0;
    setDirty(false);
    setSaveState("saved");
    hydratedId.current = `${serverDraft.orchestration_id}:${serverDraft.updated_at_ms}`;
    const snapshot = orchestrationGraphSnapshot(serverDraft);
    setHistory([snapshot]);
    setHistoryIndex(0);
  }, [aggregateQuery.data?.draft, aggregateQuery.data?.latest_published, dirty]);

  useEffect(() => {
    setNavigationLock?.(
      dirty
        ? {
            title: "Unsaved orchestration changes",
            message:
              "Wait for autosave or discard the draft changes before leaving Orchestrations.",
            showOverlay: false,
          }
        : null
    );
    return () => setNavigationLock?.(null);
  }, [dirty, setNavigationLock]);

  const saveDraft = useCallback(
    async (value: OrchestrationSpec) => {
      if (saveInFlightRef.current || readOnlySnapshot) return null;
      saveInFlightRef.current = true;
      const savedRevision = revisionRef.current;
      const savedOrchestrationId = value.orchestration_id;
      setSaveState("saving");
      try {
        const response = await client.orchestrations.updateDraft(value.orchestration_id, {
          name: value.name,
          description: value.description,
          root_node_id: value.root_node_id,
          nodes: value.nodes,
          edges: value.edges,
          goal_policy: value.goal_policy,
          metadata: value.metadata,
          expected_updated_at_ms: value.updated_at_ms,
        });
        const changedWhileSaving = revisionRef.current !== savedRevision;
        const stillOpen = selectedIdRef.current === savedOrchestrationId;
        if (stillOpen) {
          setDraft((current) => {
            if (current?.orchestration_id !== savedOrchestrationId) return current;
            return changedWhileSaving
              ? { ...current, updated_at_ms: response.orchestration.updated_at_ms }
              : response.orchestration;
          });
          setDirty(changedWhileSaving);
          setSaveState("saved");
          hydratedId.current = `${response.orchestration_id}:${response.updated_at_ms}`;
        }
        await queryClient.invalidateQueries({ queryKey: ["orchestrations"] });
        return changedWhileSaving || !stillOpen ? null : response.orchestration;
      } catch (error: any) {
        const conflict = String(error?.message || "")
          .toLowerCase()
          .includes("conflict");
        if (selectedIdRef.current === savedOrchestrationId) {
          setSaveState(conflict ? "conflict" : "error");
          if (!conflict) toast("err", error?.message || "Could not save orchestration draft");
        }
        return null;
      } finally {
        saveInFlightRef.current = false;
      }
    },
    [client, queryClient, readOnlySnapshot, toast]
  );

  useEffect(() => {
    if (readOnlySnapshot || !dirty || !draft || saveState !== "saved")
      return;
    const timer = window.setTimeout(() => void saveDraft(draft), 1200);
    return () => window.clearTimeout(timer);
  }, [dirty, draft, readOnlySnapshot, saveDraft, saveState]);

  const mutateDraft = useCallback(
    (recipe: (current: OrchestrationSpec) => OrchestrationSpec, recordHistory = true) => {
      if (readOnlySnapshot) return;
      revisionRef.current += 1;
      setDraft((current) => {
        if (!current) return current;
        const next = synchronizeWorkflowTransitionKeys(recipe(current));
        if (recordHistory) {
          const snapshot = orchestrationGraphSnapshot(next);
          setHistory((items) => [...items.slice(0, historyIndex + 1), snapshot].slice(-50));
          setHistoryIndex((index) => Math.min(index + 1, 49));
        }
        return next;
      });
      setDirty(true);
      setSaveState("saved");
    },
    [historyIndex, readOnlySnapshot]
  );

  const restoreHistory = useCallback(
    (index: number) => {
      const snapshot = history[index];
      if (!draft || !snapshot) return;
      const value = JSON.parse(snapshot);
      revisionRef.current += 1;
      setDraft({ ...draft, ...value });
      setHistoryIndex(index);
      setDirty(true);
      setSaveState("saved");
    },
    [draft, history]
  );

  useEffect(() => {
    const listener = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey)) return;
      const target = event.target as HTMLElement | null;
      if (target?.matches("input, textarea, select") || target?.isContentEditable) return;
      if (event.key.toLowerCase() === "z") {
        event.preventDefault();
        restoreHistory(event.shiftKey ? historyIndex + 1 : historyIndex - 1);
      }
      if (event.key.toLowerCase() === "s" && draft) {
        event.preventDefault();
        void saveDraft(draft);
      }
      if (event.key.toLowerCase() === "c" && selection?.kind === "node" && draft) {
        copiedNode.current = draft.nodes.find((node) => node.node_id === selection.id) || null;
      }
      if (event.key.toLowerCase() === "v" && copiedNode.current) {
        event.preventDefault();
        const source = copiedNode.current;
        const node = {
          ...source,
          node_id: orchestrationEditorId(source.kind),
          name: `${source.name} copy`,
          position: { x: source.position.x + 32, y: source.position.y + 32 },
        } as OrchestrationNodeSpec;
        mutateDraft((current) => ({ ...current, nodes: [...current.nodes, node] }));
        setSelection({ kind: "node", id: node.node_id });
      }
    };
    window.addEventListener("keydown", listener);
    return () => window.removeEventListener("keydown", listener);
  }, [draft, historyIndex, mutateDraft, restoreHistory, saveDraft, selection]);

  const workflows = useMemo(
    () => (automationsQuery.data?.automations || []).filter((row: any) => automationId(row)),
    [automationsQuery.data]
  );
  const activeGoals = useMemo(() => {
    const counts = new Map<string, number>();
    (goalsQuery.data?.goals || []).forEach((goal) => {
      if (!["completed", "failed", "cancelled", "expired"].includes(goal.status)) {
        counts.set(goal.orchestration_id, (counts.get(goal.orchestration_id) || 0) + 1);
      }
    });
    return counts;
  }, [goalsQuery.data]);
  const filtered = useMemo(() => {
    const needle = search.trim().toLowerCase();
    return summaries.filter((summary) => {
      if (needle && !`${summary.name} ${summary.orchestration_id}`.toLowerCase().includes(needle))
        return false;
      if (filter === "draft") return summary.draft?.status === "draft";
      if (filter === "published") return summary.latest_published_version !== null;
      if (filter === "archived") return summary.draft?.status === "archived";
      if (filter === "invalid") return statusById.get(summary.orchestration_id)?.valid === false;
      if (filter === "stale") return (statusById.get(summary.orchestration_id)?.stale || 0) > 0;
      return true;
    });
  }, [filter, search, statusById, summaries]);

  const createMutation = useMutation({
    mutationFn: async () => {
      const workflow = workflows[0];
      if (!workflow) throw new Error("Create an Automation V2 workflow before an orchestration.");
      const root = createOrchestrationNode(`workflow:${automationId(workflow)}`);
      root.name = automationName(workflow);
      return client.orchestrations.create({
        name: newName.trim() || "Untitled orchestration",
        root_node_id: root.node_id,
        nodes: [root],
        goal_policy: { max_hops: 20, on_limit: "pause_for_review" },
      });
    },
    onSuccess: async (response) => {
      setShowCreate(false);
      setNewName("");
      setSelectedId(response.orchestration_id);
      await queryClient.invalidateQueries({ queryKey: ["orchestrations"] });
      toast("ok", "Orchestration draft created");
    },
    onError: (error: any) => toast("err", error?.message || "Could not create orchestration"),
  });
  const operationMutation = useMutation({
    mutationFn: async (operation: "validate" | "publish" | "archive" | "refresh") => {
      if (!selectedId || !draft) throw new Error("Select an orchestration draft first.");
      let persisted = draft;
      if (dirty) {
        const saved = await saveDraft(draft);
        if (!saved) {
          throw new Error("Save the current draft successfully before running this action.");
        }
        persisted = saved;
      }
      if (operation === "validate") return client.orchestrations.validate(selectedId);
      if (operation === "publish") return client.orchestrations.publish(selectedId, persisted.updated_at_ms);
      if (operation === "archive") return client.orchestrations.archive(selectedId, persisted.updated_at_ms);
      return client.orchestrations.refreshReferences(selectedId, persisted.updated_at_ms);
    },
    onSuccess: async (_, operation) => {
      await queryClient.invalidateQueries({ queryKey: ["orchestrations"] });
      toast(
        "ok",
        operation === "publish" ? "Immutable version published" : "Orchestration updated"
      );
      if (operation === "archive") setSelectedId("");
    },
    onError: (error: any) => {
      const conflict = String(error?.message || "").toLowerCase().includes("conflict");
      if (conflict) setDirty(true);
      if (conflict) setSaveState("conflict");
      toast("err", error?.message || "Orchestration action failed");
    },
  });
  const duplicateMutation = useMutation({
    mutationFn: async () => {
      if (!draft) throw new Error("Select an orchestration draft first.");
      return client.orchestrations.create({
        name: `${draft.name} copy`,
        description: draft.description,
        root_node_id: draft.root_node_id,
        nodes: draft.nodes,
        edges: draft.edges,
        goal_policy: draft.goal_policy,
        metadata: draft.metadata,
      });
    },
    onSuccess: async (response) => {
      setSelectedId(response.orchestration_id);
      hydratedId.current = "";
      await queryClient.invalidateQueries({ queryKey: ["orchestrations"] });
      toast("ok", "Orchestration duplicated as a new draft");
    },
    onError: (error: any) => toast("err", error?.message || "Could not duplicate orchestration"),
  });
  const startGoalMutation = useMutation({
    mutationFn: () => {
      if (!selectedId) throw new Error("Select an orchestration first.");
      return client.statefulRuntime.startGoal({
        orchestrationId: selectedId,
        objective: goalObjective.trim(),
        idempotencyKey: orchestrationEditorId("control-panel-goal"),
      });
    },
    onSuccess: () => {
      setGoalObjective("");
      toast("ok", "Long-running goal started");
      void queryClient.invalidateQueries({ queryKey: ["orchestrations", "goals"] });
      navigate("runs");
    },
    onError: (error: any) => toast("err", error?.message || "Could not start goal"),
  });

  const clientValidation = useMemo(
    () => (draft ? validateOrchestrationDraft(draft) : { valid: false, issues: [] }),
    [draft]
  );
  const validationIssues = useMemo(() => {
    const serverIssues = readOnlySnapshot ? [] : validationQuery.data?.report.issues || [];
    const combined = [...clientValidation.issues, ...serverIssues];
    return combined.filter(
      (issue, index) =>
        combined.findIndex(
          (candidate) =>
            candidate.code === issue.code &&
            candidate.node_id === issue.node_id &&
            candidate.edge_id === issue.edge_id
        ) === index
    );
  }, [clientValidation.issues, readOnlySnapshot, validationQuery.data?.report.issues]);
  const issueCounts = useMemo(() => {
    const counts = new Map<string, number>();
    validationIssues.forEach((issue) => {
      if (issue.node_id) counts.set(issue.node_id, (counts.get(issue.node_id) || 0) + 1);
    });
    return counts;
  }, [validationIssues]);
  const staleNodeIds = useMemo(
    () =>
      new Set(
        (staleQuery.data?.references || [])
          .filter((row) => row.state !== "fresh")
          .map((row) => row.node_id)
      ),
    [staleQuery.data]
  );
  const referencesNeedRefresh =
    staleQuery.data?.references.some((reference) => reference.state !== "fresh") ?? false;

  const addNode = useCallback(
    (kind: string, position?: { x: number; y: number }) => {
      const index = draft?.nodes.length || 0;
      const resolvedPosition = position || {
        x: 120 + (index % 3) * 260,
        y: 120 + Math.floor(index / 3) * 150,
      };
      const node = createOrchestrationNode(kind, resolvedPosition);
      const workflowId = node.kind === "workflow" ? node.automation_id : "";
      const workflow = workflowId
        ? workflows.find((row: any) => automationId(row) === workflowId)
        : null;
      if (workflow) node.name = automationName(workflow);
      mutateDraft((current) => ({ ...current, nodes: [...current.nodes, node] }));
      setSelection({ kind: "node", id: node.node_id });
      setSelectedNodeIds(new Set([node.node_id]));
      setSelectedEdgeIds(new Set());
    },
    [draft?.nodes.length, mutateDraft, workflows]
  );

  const onNodesChange = useCallback(
    (changes: NodeChange[]) => {
      if (!draft) return;
      const graphChanges = changes.filter(
        (change) => change.type === "position" || change.type === "remove"
      );
      if (!graphChanges.length) return;
      const nextCanvas = applyNodeChanges(
        graphChanges,
        toCanvasNodes(draft.nodes, draft.root_node_id, issueCounts, staleNodeIds)
      );
      mutateDraft(
        (current) => ({
          ...current,
          nodes: current.nodes
            .filter((node) => nextCanvas.some((canvas) => canvas.id === node.node_id))
            .map((node) => ({
              ...node,
              position:
                nextCanvas.find((canvas) => canvas.id === node.node_id)?.position || node.position,
            })),
          edges: current.edges.filter(
            (edge) =>
              nextCanvas.some((node) => node.id === edge.from_node_id) &&
              nextCanvas.some((node) => node.id === edge.to_node_id)
          ),
        }),
        graphChanges.some(
          (change) => change.type === "remove" || (change.type === "position" && !change.dragging)
        )
      );
    },
    [draft, issueCounts, mutateDraft, staleNodeIds]
  );
  const onEdgesChange = useCallback(
    (changes: EdgeChange[]) => {
      if (!draft) return;
      const graphChanges = changes.filter((change) => change.type === "remove");
      if (!graphChanges.length) return;
      const next = applyEdgeChanges(graphChanges, toCanvasEdges(draft.edges));
      mutateDraft((current) => ({
        ...current,
        edges: current.edges.filter((edge) => next.some((item) => item.id === edge.edge_id)),
      }));
    },
    [draft, mutateDraft]
  );
  const onConnect = useCallback(
    (connection: Connection) => {
      if (!connection.source || !connection.target || connection.source === connection.target)
        return;
      const edge: OrchestrationEdgeSpec = {
        edge_id: orchestrationEditorId("transition"),
        from_node_id: connection.source,
        to_node_id: connection.target,
        transition_key: `transition_${(draft?.edges.length || 0) + 1}`,
      };
      mutateDraft((current) => ({
        ...current,
        nodes: current.nodes.map((node) =>
          node.node_id === edge.from_node_id && node.kind === "workflow"
            ? {
                ...node,
                allowed_transition_keys: Array.from(
                  new Set([...(node.allowed_transition_keys || []), edge.transition_key])
                ),
              }
            : node
        ),
        edges: [...current.edges, edge],
      }));
      setSelection({ kind: "edge", id: edge.edge_id });
      setSelectedNodeIds(new Set());
      setSelectedEdgeIds(new Set([edge.edge_id]));
    },
    [draft?.edges.length, mutateDraft]
  );
  const onReconnect = useCallback(
    (edgeId: string, connection: Connection) => {
      if (!connection.source || !connection.target || connection.source === connection.target)
        return;
      mutateDraft((current) => ({
        ...current,
        edges: current.edges.map((edge) =>
          edge.edge_id === edgeId
            ? {
                ...edge,
                from_node_id: connection.source!,
                to_node_id: connection.target!,
              }
            : edge
        ),
      }));
    },
    [mutateDraft]
  );

  const deleteSelection = useCallback(() => {
    if (!selection) return;
    mutateDraft((current) => {
      if (selection.kind === "edge")
        return { ...current, edges: current.edges.filter((edge) => edge.edge_id !== selection.id) };
      const nodes = current.nodes.filter((node) => node.node_id !== selection.id);
      return {
        ...current,
        root_node_id:
          current.root_node_id === selection.id
            ? nodes.find((node) => node.kind === "workflow")?.node_id || ""
            : current.root_node_id,
        nodes,
        edges: current.edges.filter(
          (edge) => edge.from_node_id !== selection.id && edge.to_node_id !== selection.id
        ),
      };
    });
    setSelection(null);
  }, [mutateDraft, selection]);

  const returnToLibrary = useCallback(async () => {
    if (dirty && draft) {
      const saved = await saveDraft(draft);
      if (!saved) {
        toast(
          "warn",
          "Resolve the save conflict or preserve these edits as a copy before leaving."
        );
        return;
      }
    }
    hydratedId.current = "";
    setSelectedId("");
    setDraft(null);
    setSelection(null);
    setSelectedNodeIds(new Set());
    setSelectedEdgeIds(new Set());
  }, [dirty, draft, saveDraft, toast]);

  if (selectedId && (aggregateQuery.isLoading || !draft)) {
    return (
      <div className="grid min-h-[50vh] place-items-center text-sm text-[var(--color-text-subtle)]">
        Loading orchestration…
      </div>
    );
  }

  if (selectedId && draft) {
    const graphAnalysis = analyzeGraph(draft);
    const graph = graphAnalysis.counts;
    const loopNodeIds = new Set(graphAnalysis.loopComponents.flat());
    const loopEdgeIds = new Set(
      draft.edges
        .filter((edge) =>
          graphAnalysis.loopComponents.some(
            (component) =>
              component.includes(edge.from_node_id) && component.includes(edge.to_node_id)
          )
        )
        .map((edge) => edge.edge_id)
    );
    const published = aggregateQuery.data?.latest_published;
    const comparisonChanges = compareOrchestrationSpecs(draft, published || null);
    const graphValid =
      clientValidation.valid &&
      (readOnlySnapshot ||
        (validationQuery.data?.report.valid === true &&
          staleQuery.data !== undefined &&
          !referencesNeedRefresh));
    const canPublish = !readOnlySnapshot && graphValid && !dirty && saveState === "saved";
    return (
      <div className="orch-studio-page">
        <header className="orch-studio-toolbar">
          <div className="flex min-w-0 items-center gap-3">
            <button
              className="tcp-icon-btn"
              title="Back to Orchestrations"
              aria-label="Back to Orchestrations"
              onClick={() => void returnToLibrary()}
            >
              <Icon name="chevron-left" />
            </button>
            <div className="min-w-0">
              <input
                className="orch-title-input"
                aria-label="Orchestration name"
                value={draft.name}
                readOnly={readOnlySnapshot}
                onInput={(event) =>
                  mutateDraft((current) => ({ ...current, name: event.currentTarget.value }))
                }
              />
              <div className="flex flex-wrap items-center gap-2 text-xs text-[var(--color-text-subtle)]">
                <span>
                  {publishedOnly ? "Published snapshot" : archivedDraft ? "Archived draft" : "Draft"}
                </span>
                <span>Version {draft.version}</span>
                <span>{graph.workflows} workflows</span>
                <span aria-live="polite">
                  {saveState === "saving"
                    ? "Saving…"
                    : saveState === "conflict"
                      ? "Save conflict"
                      : dirty
                        ? "Unsaved"
                        : "Saved"}
                </span>
              </div>
            </div>
          </div>
          <div className="flex flex-wrap items-center justify-end gap-2">
            <button
              className="tcp-icon-btn"
              title="Undo"
              aria-label="Undo"
              disabled={readOnlySnapshot || historyIndex <= 0}
              onClick={() => restoreHistory(historyIndex - 1)}
            >
              <Icon name="rotate-ccw" />
            </button>
            <button
              className="tcp-icon-btn"
              title="Redo"
              aria-label="Redo"
              disabled={readOnlySnapshot || historyIndex >= history.length - 1}
              onClick={() => restoreHistory(historyIndex + 1)}
            >
              <Icon name="rotate-cw" />
            </button>
            <button
              className="tcp-btn"
              disabled={readOnlySnapshot}
              onClick={() =>
                mutateDraft((current) =>
                  autoLayoutLeftToRight(current, {
                    origin: { x: 72, y: 180 },
                    rankSpacing: 290,
                    nodeSpacing: 160,
                  })
                )
              }
            >
              <Icon name="layout-dashboard" />
              Auto layout
            </button>
            <button
              className="tcp-btn"
              disabled={duplicateMutation.isPending}
              onClick={() => duplicateMutation.mutate()}
            >
              <Icon name="copy" />
              Duplicate
            </button>
            <button
              className="tcp-btn"
              disabled={readOnlySnapshot || saveState !== "saved" ||
                !referencesNeedRefresh || operationMutation.isPending}
              onClick={() => operationMutation.mutate("refresh")}
            >
              <Icon name="refresh-cw" />
              Refresh refs
            </button>
            <button
              className="tcp-btn"
              disabled={readOnlySnapshot || saveState !== "saved" || operationMutation.isPending}
              onClick={() => operationMutation.mutate("validate")}
            >
              <Icon name="shield-check" />
              Validate
            </button>
            <button
              className="tcp-btn-primary"
              disabled={!canPublish || operationMutation.isPending}
              onClick={() => {
                if (
                  window.confirm(
                    "Publish this validated draft as a new immutable orchestration version?"
                  )
                )
                  operationMutation.mutate("publish");
              }}
            >
              <Icon name="rocket" />
              Publish
            </button>
            <button
              className="tcp-icon-btn"
              title="Archive draft"
              aria-label="Archive draft"
              disabled={readOnlySnapshot || saveState !== "saved" || operationMutation.isPending}
              onClick={() => {
                if (window.confirm("Archive this orchestration draft?"))
                  operationMutation.mutate("archive");
              }}
            >
              <Icon name="archive" />
            </button>
          </div>
        </header>

        {saveState === "conflict" ? (
          <div className="orch-conflict" role="alert">
            <Icon name="triangle-alert" />
            <span>
              The draft changed elsewhere. Preserve these edits as a new draft, or reload the server
              copy.
            </span>
            <button
              className="tcp-btn-primary"
              disabled={duplicateMutation.isPending}
              onClick={() => duplicateMutation.mutate()}
            >
              <Icon name="copy" />
              Save as copy
            </button>
            <button
              className="tcp-btn"
              onClick={() => {
                setDirty(false);
                hydratedId.current = "";
                void aggregateQuery.refetch();
              }}
            >
              Reload server copy
            </button>
          </div>
        ) : null}
        {saveState === "error" ? (
          <div className="orch-conflict" role="alert">
            <Icon name="triangle-alert" />
            <span>Autosave failed. Your edits remain local until you retry, preserve a copy, or reload.</span>
            <button className="tcp-btn-primary" onClick={() => void saveDraft(draft)}>
              <Icon name="refresh-cw" />
              Retry save
            </button>
            <button
              className="tcp-btn"
              disabled={duplicateMutation.isPending}
              onClick={() => duplicateMutation.mutate()}
            >
              <Icon name="copy" />
              Save as copy
            </button>
            <button
              className="tcp-btn"
              onClick={() => {
                setDirty(false);
                setSaveState("saved");
                hydratedId.current = "";
                void aggregateQuery.refetch();
              }}
            >
              Reload server copy
            </button>
          </div>
        ) : null}

        <div className="orch-studio-grid">
          <aside className="orch-palette" aria-label="Node palette">
            <fieldset disabled={readOnlySnapshot} className="contents">
              <div className="border-b border-[var(--color-border-subtle)] p-3">
                <div className="mb-2 text-xs font-semibold uppercase text-[var(--color-text-subtle)]">
                  Add nodes
                </div>
                <div className="relative">
                  <Icon
                    name="search"
                    className="absolute left-2.5 top-2.5 text-[var(--color-text-subtle)]"
                  />
                  <SearchInput
                    aria-label="Search workflows"
                    className="tcp-input w-full pl-8"
                    placeholder="Search workflows"
                    value={paletteSearch}
                    onInput={(event) => setPaletteSearch(event.currentTarget.value)}
                  />
                </div>
              </div>
              <div className="grid gap-1 overflow-y-auto p-3">
                <div className="orch-palette-heading">Workflows</div>
                {workflows
                  .filter((row: any) =>
                    automationName(row).toLowerCase().includes(paletteSearch.toLowerCase())
                  )
                  .map((row: any) => (
                    <PaletteItem
                      key={automationId(row)}
                      kind={`workflow:${automationId(row)}`}
                      label={automationName(row)}
                      icon="workflow"
                      onAdd={addNode}
                    />
                  ))}
                <div className="orch-palette-heading mt-3">Waits</div>
                <PaletteItem kind="wait:timer" label="Timer" icon="clock" onAdd={addNode} />
                <PaletteItem
                  kind="wait:approval"
                  label="Approval"
                  icon="shield-check"
                  onAdd={addNode}
                />
                <PaletteItem kind="wait:webhook" label="Webhook" icon="webhook" onAdd={addNode} />
                <PaletteItem
                  kind="wait:external_condition"
                  label="External condition"
                  icon="radio"
                  onAdd={addNode}
                />
                <div className="orch-palette-heading mt-3">Terminals</div>
                <PaletteItem
                  kind="terminal:complete"
                  label="Complete"
                  icon="square-check-big"
                  onAdd={addNode}
                />
                <PaletteItem
                  kind="terminal:pause"
                  label="Pause"
                  icon="pause-circle"
                  onAdd={addNode}
                />
                <PaletteItem kind="terminal:fail" label="Fail" icon="x-circle" onAdd={addNode} />
              </div>
            </fieldset>
          </aside>

          <main className="min-w-0 overflow-hidden">
            <div className="orch-mode-tabs" role="tablist" aria-label="Authoring mode">
              <button
                role="tab"
                aria-selected={editorMode === "canvas"}
                className={editorMode === "canvas" ? "active" : ""}
                onClick={() => setEditorMode("canvas")}
              >
                <Icon name="network" />
                Canvas
              </button>
              <button
                role="tab"
                aria-selected={editorMode === "outline"}
                className={editorMode === "outline" ? "active" : ""}
                onClick={() => setEditorMode("outline")}
              >
                <Icon name="list-tree" />
                Outline
              </button>
            </div>
            {editorMode === "canvas" ? (
              <OrchestrationCanvas
                nodes={draft.nodes}
                edges={draft.edges}
                rootNodeId={draft.root_node_id}
                issueCounts={issueCounts}
                staleNodeIds={staleNodeIds}
                loopNodeIds={loopNodeIds}
                loopEdgeIds={loopEdgeIds}
                selectedNodeIds={selectedNodeIds}
                selectedEdgeIds={selectedEdgeIds}
                onSelectionChange={(next) => {
                  setSelection(next);
                  setSelectedNodeIds(new Set(next?.kind === "node" ? [next.id] : []));
                  setSelectedEdgeIds(new Set(next?.kind === "edge" ? [next.id] : []));
                }}
                onSelectionSetChange={(nodeIds, edgeIds) => {
                  if (!nodeIds.length && !edgeIds.length && selection) return;
                  setSelectedNodeIds((current) =>
                    sameStringSet(current, nodeIds) ? current : new Set(nodeIds)
                  );
                  setSelectedEdgeIds((current) =>
                    sameStringSet(current, edgeIds) ? current : new Set(edgeIds)
                  );
                  const lastNode = nodeIds.at(-1);
                  const lastEdge = edgeIds.at(-1);
                  const next: OrchestrationSelection = lastNode
                    ? { kind: "node", id: lastNode }
                    : lastEdge
                      ? { kind: "edge", id: lastEdge }
                      : null;
                  setSelection((current) =>
                    current?.kind === next?.kind && current?.id === next?.id ? current : next
                  );
                }}
                onNodesChange={onNodesChange}
                onEdgesChange={onEdgesChange}
                onConnect={onConnect}
                onReconnect={onReconnect}
                onDropNode={addNode}
                readOnly={readOnlySnapshot}
              />
            ) : (
              <div className="orch-outline" role="tabpanel">
                <div className="grid gap-2">
                  {draft.nodes.map((node, index) => (
                    <div
                      key={node.node_id}
                      className={`orch-outline-row ${selection?.kind === "node" && selection.id === node.node_id ? "selected" : ""}`}
                    >
                      <button
                        className="flex min-w-0 flex-1 items-center gap-3"
                        onClick={() => setSelection({ kind: "node", id: node.node_id })}
                      >
                        <span className="orch-outline-index">{index + 1}</span>
                        <span className="min-w-0 flex-1 text-left">
                          <strong className="block truncate">{node.name}</strong>
                          <span className="text-xs text-[var(--color-text-subtle)]">
                            {node.kind}
                            {node.node_id === draft.root_node_id ? " · root" : ""}
                          </span>
                        </span>
                        <span className="text-xs text-[var(--color-text-subtle)]">
                          {draft.edges.filter((edge) => edge.from_node_id === node.node_id).length}{" "}
                          outgoing
                        </span>
                      </button>
                      <button
                        className="tcp-icon-btn"
                        aria-label={`Move ${node.name} up`}
                        disabled={readOnlySnapshot || index === 0}
                        onClick={() =>
                          mutateDraft((current) => {
                            const nodes = [...current.nodes];
                            const [moved] = nodes.splice(index, 1);
                            nodes.splice(index - 1, 0, moved);
                            return { ...current, nodes };
                          })
                        }
                      >
                        <Icon name="arrow-up" />
                      </button>
                      <button
                        className="tcp-icon-btn"
                        aria-label={`Move ${node.name} down`}
                        disabled={readOnlySnapshot || index === draft.nodes.length - 1}
                        onClick={() =>
                          mutateDraft((current) => {
                            const nodes = [...current.nodes];
                            const [moved] = nodes.splice(index, 1);
                            nodes.splice(index + 1, 0, moved);
                            return { ...current, nodes };
                          })
                        }
                      >
                        <Icon name="arrow-down" />
                      </button>
                    </div>
                  ))}
                </div>
                <div className="mt-5 grid gap-2">
                  <div className="text-xs font-semibold uppercase text-[var(--color-text-subtle)]">
                    Named transitions
                  </div>
                  {draft.edges.map((edge) => (
                    <button
                      key={edge.edge_id}
                      className={`orch-outline-row ${selection?.kind === "edge" && selection.id === edge.edge_id ? "selected" : ""}`}
                      onClick={() => setSelection({ kind: "edge", id: edge.edge_id })}
                    >
                      <Icon name="arrow-right" />
                      <span className="font-mono text-xs">{edge.transition_key}</span>
                      <span className="ml-auto text-xs text-[var(--color-text-subtle)]">
                        {edge.from_node_id} → {edge.to_node_id}
                      </span>
                    </button>
                  ))}
                  <fieldset
                    className="orch-transition-form"
                    aria-label="Add named transition"
                    disabled={readOnlySnapshot}
                  >
                    <select
                      className="tcp-input"
                      aria-label="Transition source"
                      value={transitionSource}
                      onChange={(event) => setTransitionSource(event.currentTarget.value)}
                    >
                      <option value="">Source</option>
                      {draft.nodes
                        .filter((node) => node.kind !== "terminal")
                        .map((node) => (
                          <option key={node.node_id} value={node.node_id}>
                            {node.name}
                          </option>
                        ))}
                    </select>
                    <select
                      className="tcp-input"
                      aria-label="Transition target"
                      value={transitionTarget}
                      onChange={(event) => setTransitionTarget(event.currentTarget.value)}
                    >
                      <option value="">Target</option>
                      {draft.nodes.map((node) => (
                        <option key={node.node_id} value={node.node_id}>
                          {node.name}
                        </option>
                      ))}
                    </select>
                    <input
                      className="tcp-input"
                      aria-label="Transition key"
                      placeholder="Named outcome"
                      value={transitionKey}
                      onInput={(event) => setTransitionKey(event.currentTarget.value)}
                    />
                    <button
                      className="tcp-btn"
                      disabled={!transitionSource || !transitionTarget || !transitionKey.trim()}
                      onClick={() => {
                        const edge = {
                          edge_id: orchestrationEditorId("transition"),
                          from_node_id: transitionSource,
                          to_node_id: transitionTarget,
                          transition_key: transitionKey.trim(),
                        };
                        mutateDraft((current) => ({
                          ...current,
                          nodes: current.nodes.map((node) =>
                            node.node_id === edge.from_node_id && node.kind === "workflow"
                              ? {
                                  ...node,
                                  allowed_transition_keys: Array.from(
                                    new Set([
                                      ...(node.allowed_transition_keys || []),
                                      edge.transition_key,
                                    ])
                                  ),
                                }
                              : node
                          ),
                          edges: [...current.edges, edge],
                        }));
                        setTransitionKey("");
                        setSelection({ kind: "edge", id: edge.edge_id });
                      }}
                    >
                      <Icon name="plus" />
                      Add transition
                    </button>
                  </fieldset>
                </div>
              </div>
            )}
          </main>

          <OrchestrationInspector
            selection={selection}
            nodes={draft.nodes}
            edges={draft.edges}
            rootNodeId={draft.root_node_id}
            policy={draft.goal_policy}
            onNodeChange={(node) =>
              mutateDraft((current) => ({
                ...current,
                nodes: current.nodes.map((item) => (item.node_id === node.node_id ? node : item)),
              }))
            }
            onEdgeChange={(edge) =>
              mutateDraft((current) => ({
                ...current,
                nodes: current.nodes.map((node) =>
                  node.node_id === edge.from_node_id && node.kind === "workflow"
                    ? {
                        ...node,
                        allowed_transition_keys: Array.from(
                          new Set([...(node.allowed_transition_keys || []), edge.transition_key])
                        ),
                      }
                    : node
                ),
                edges: current.edges.map((item) => (item.edge_id === edge.edge_id ? edge : item)),
              }))
            }
            onPolicyChange={(goal_policy) =>
              mutateDraft((current) => ({ ...current, goal_policy }))
            }
            onSetRoot={(root_node_id) => mutateDraft((current) => ({ ...current, root_node_id }))}
            onDelete={deleteSelection}
            readOnly={readOnlySnapshot}
          />
        </div>

        <section className="orch-bottom-panel">
          <div className="grid gap-3 xl:grid-cols-[1.1fr_0.8fr_0.9fr]">
            <div>
              <div className="mb-2 flex items-center justify-between">
                <strong className="text-sm">Validation</strong>
                <span className={`orch-validation-state ${archivedDraft ? "unchecked" : graphValid ? "valid" : "invalid"}`}>
                  {archivedDraft ? "Not checked" : validationQuery.isFetching ? "Checking" :
                    graphValid ? "Valid" : `${validationIssues.length} issues`}
                </span>
              </div>
              <div className="grid max-h-32 gap-1 overflow-y-auto">
                {validationIssues.length ? (
                  validationIssues.map((issue: OrchestrationValidationIssue, index) => (
                    <button
                      key={`${issue.code}-${index}`}
                      className="orch-validation-row"
                      onClick={() =>
                        setSelection(
                          issue.node_id
                            ? { kind: "node", id: issue.node_id }
                            : issue.edge_id
                              ? { kind: "edge", id: issue.edge_id }
                              : null
                        )
                      }
                    >
                      <Icon name="triangle-alert" />
                      <span>
                        <strong>{issue.code}</strong> {issue.message}
                      </span>
                    </button>
                  ))
                ) : (
                  <div className="text-xs text-[var(--color-text-subtle)]">
                    {archivedDraft ? "Server validation is not run for archived drafts." :
                      "No server validation issues."}
                  </div>
                )}
              </div>
            </div>
            <div className="grid gap-2">
              <strong className="text-sm">Dry-run transition</strong>
              <div className="grid grid-cols-2 gap-2">
                <select
                  className="tcp-input"
                  aria-label="Dry-run source node"
                  value={previewFrom}
                  onChange={(event) => setPreviewFrom(event.currentTarget.value)}
                >
                  <option value="">Source node</option>
                  {draft.nodes
                    .filter((node) => node.kind === "workflow")
                    .map((node) => (
                      <option key={node.node_id} value={node.node_id}>
                        {node.name}
                      </option>
                    ))}
                </select>
                <input
                  className="tcp-input"
                  placeholder="Transition key"
                  value={previewKey}
                  onInput={(event) => setPreviewKey(event.currentTarget.value)}
                />
              </div>
              <button
                className="tcp-btn"
                disabled={dirty || saveState !== "saved" || !previewFrom || !previewKey}
                onClick={async () => {
                  try {
                    setPreviewResult(
                      await client.orchestrations.previewTransition(selectedId, {
                        fromNodeId: previewFrom,
                        transitionKey: previewKey,
                        version: publishedOnly ? draft.version : undefined,
                      })
                    );
                  } catch (error: any) {
                    toast("err", error?.message || "Dry run failed");
                  }
                }}
              >
                <Icon name="play" />
                Preview
              </button>
              {previewResult ? (
                <div className="text-xs" role="status">
                  {previewResult.allowed
                    ? `Allowed → ${previewResult.target?.name || "target"}`
                    : `Blocked: ${previewResult.issues?.[0]?.code || "invalid transition"}`}
                </div>
              ) : null}
            </div>
            <div className="grid gap-2">
              <div className="flex items-center justify-between">
                <strong className="text-sm">Versions and goals</strong>
                <span className="text-xs text-[var(--color-text-subtle)]">
                  {comparisonChanges.length} draft changes
                </span>
              </div>
              <div className="flex flex-wrap gap-1 text-xs text-[var(--color-text-subtle)]">
                {versionsQuery.data?.versions.length ? (
                  versionsQuery.data.versions.map((version) => (
                    <span className="orch-version-chip" key={version.version}>
                      v{version.version} · {formatOrchestrationTime(version.published_at_ms)}
                    </span>
                  ))
                ) : (
                  <span>No published versions</span>
                )}
              </div>
              <details className="text-xs">
                <summary className="cursor-pointer font-medium">
                  Compare with {published ? `v${published.version}` : "initial draft"}
                </summary>
                <ul className="mt-2 grid max-h-24 gap-1 overflow-y-auto text-[var(--color-text-subtle)]">
                  {comparisonChanges.map((change) => (
                    <li key={change}>• {change}</li>
                  ))}
                </ul>
              </details>
              <div className="flex gap-2">
                <input
                  className="tcp-input min-w-0 flex-1"
                  aria-label="Goal objective"
                  placeholder="Goal objective"
                  value={goalObjective}
                  onInput={(event) => setGoalObjective(event.currentTarget.value)}
                />
                <button
                  className="tcp-btn-primary"
                  disabled={
                    !goalObjective.trim() ||
                    !versionsQuery.data?.versions.length ||
                    startGoalMutation.isPending
                  }
                  onClick={() => startGoalMutation.mutate()}
                >
                  <Icon name="play" />
                  Start goal
                </button>
              </div>
            </div>
          </div>
        </section>
      </div>
    );
  }

  return (
    <div className="grid gap-5 p-1">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <div className="tcp-page-eyebrow">Long-running goals</div>
          <h1 className="tcp-page-title">Orchestrations</h1>
        </div>
        <button className="tcp-btn-primary" onClick={() => setShowCreate(true)}>
          <Icon name="plus" />
          New orchestration
        </button>
      </div>
      <div className="flex flex-wrap items-center gap-2">
        <div className="relative min-w-[220px] flex-1">
          <Icon name="search" className="absolute left-3 top-2.5 text-[var(--color-text-subtle)]" />
          <SearchInput
            aria-label="Search orchestrations"
            className="tcp-input w-full pl-9"
            placeholder="Search orchestrations"
            value={search}
            onInput={(event) => setSearch(event.currentTarget.value)}
          />
        </div>
        <div className="orch-filter-tabs" role="tablist" aria-label="Orchestration filters">
          {(["all", "draft", "published", "archived", "invalid", "stale"] as LibraryFilter[]).map(
            (value) => (
              <button
                key={value}
                role="tab"
                aria-selected={filter === value}
                className={filter === value ? "active" : ""}
                onClick={() => setFilter(value)}
              >
                {value[0].toUpperCase() + value.slice(1)}
              </button>
            )
          )}
        </div>
      </div>
      {listQuery.isError ? (
        <div className="orch-error-state" role="alert">
          <Icon name="triangle-alert" />
          <span>Orchestrations could not be loaded. Check your scope and engine connection.</span>
          <button className="tcp-btn" onClick={() => void listQuery.refetch()}>
            Retry
          </button>
        </div>
      ) : null}
      {!listQuery.isLoading && !filtered.length ? (
        <EmptyState
          title="No matching orchestrations"
          text="Create a draft or adjust the current filters."
          action={
            <button className="tcp-btn-primary" onClick={() => setShowCreate(true)}>
              Create orchestration
            </button>
          }
        />
      ) : (
        <div className="orch-library-grid">
          {filtered.map((summary: OrchestrationSummary) => {
            const status = statusById.get(summary.orchestration_id);
            const active = activeGoals.get(summary.orchestration_id) || 0;
            const counts = status?.spec ? analyzeGraph(status.spec).counts : null;
            return (
              <button
                key={summary.orchestration_id}
                className="orch-library-card"
                onClick={() => setSelectedId(summary.orchestration_id)}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="min-w-0 text-left">
                    <strong className="block truncate text-sm">{summary.name}</strong>
                    <span className="mt-1 block truncate font-mono text-xs text-[var(--color-text-subtle)]">
                      {summary.orchestration_id}
                    </span>
                  </div>
                  <span className={`orch-status ${summary.draft?.status || "published"}`}>
                    {summary.draft?.status || "published"}
                  </span>
                </div>
                <div className="mt-5 grid grid-cols-3 gap-2 text-left">
                  <div>
                    <span className="orch-metric-label">Version</span>
                    <strong>{summary.latest_published_version ?? "Draft"}</strong>
                  </div>
                  <div>
                    <span className="orch-metric-label">Goals</span>
                    <strong>{active}</strong>
                  </div>
                  <div>
                    <span className="orch-metric-label">Health</span>
                    <strong>
                      {status?.valid === false
                        ? "Invalid"
                        : status?.stale
                          ? "Stale"
                          : status?.valid === true
                            ? "Ready"
                            : "Not checked"}
                    </strong>
                  </div>
                </div>
                <div className="mt-3 text-left text-xs text-[var(--color-text-subtle)]">
                  {counts
                    ? `${counts.workflows} workflows · ${counts.waits} waits · ${counts.loops} loops`
                    : "Loading graph summary…"}
                </div>
                <div className="mt-4 flex items-center justify-between border-t border-[var(--color-border-subtle)] pt-3 text-xs text-[var(--color-text-subtle)]">
                  <span>
                    {formatOrchestrationTime(
                      summary.draft?.updated_at_ms ||
                        summary.published_versions.at(-1)?.published_at_ms
                    )}
                  </span>
                  <Icon name="chevron-right" />
                </div>
              </button>
            );
          })}
        </div>
      )}
      {showCreate ? (
        <div
          className="tcp-modal-backdrop"
          role="presentation"
          onMouseDown={() => setShowCreate(false)}
        >
          <div
            className="tcp-modal w-full max-w-md"
            role="dialog"
            aria-modal="true"
            aria-labelledby="create-orchestration-title"
            onMouseDown={(event) => event.stopPropagation()}
          >
            <div className="flex items-center justify-between">
              <h2 id="create-orchestration-title" className="text-base font-semibold">
                New orchestration
              </h2>
              <button
                className="tcp-icon-btn"
                title="Close"
                aria-label="Close"
                onClick={() => setShowCreate(false)}
              >
                <Icon name="x" />
              </button>
            </div>
            <p className="mt-2 text-sm text-[var(--color-text-subtle)]">
              The first Automation V2 workflow becomes the root. You can replace it in Studio.
            </p>
            <label className="mt-4 grid gap-1.5 text-xs font-medium">
              Name
              <input
                autoFocus
                className="tcp-input"
                value={newName}
                onInput={(event) => setNewName(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") createMutation.mutate();
                }}
              />
            </label>
            <div className="mt-5 flex justify-end gap-2">
              <button className="tcp-btn" onClick={() => setShowCreate(false)}>
                Cancel
              </button>
              <button
                className="tcp-btn-primary"
                disabled={!newName.trim() || createMutation.isPending}
                onClick={() => createMutation.mutate()}
              >
                <Icon name="plus" />
                Create draft
              </button>
            </div>
          </div>
        </div>
      ) : null}
    </div>
  );
}
