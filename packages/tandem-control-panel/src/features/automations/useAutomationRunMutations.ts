import { useEffect } from "react";
import { useMutation } from "@tanstack/react-query";

export function useAutomationRunMutations({
  client,
  toast,
  queryClient,
  selectedRunId,
  selectedBoardTaskId,
  onSelectRunId,
  onOpenRunningView,
}: any) {
  const runNowMutation = useMutation({
    mutationFn: (id: string) => client?.automations?.runNow?.(id),
    onSuccess: async () => {
      toast("ok", "Automation triggered.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const runNowV2Mutation = useMutation({
    mutationFn: async ({
      id,
      executionProfile,
    }: {
      id: string;
      executionProfile?: "strict" | "guided" | "yolo";
    }) => {
      if (!client?.automationsV2?.runNow) {
        throw new Error("Workflow run now is not available in this client.");
      }
      return client.automationsV2.runNow(id, executionProfile ? { executionProfile } : undefined);
    },
    onSuccess: async (payload: any) => {
      const runId = String(payload?.run?.run_id || payload?.run?.runId || "").trim();
      toast("ok", "Workflow automation triggered.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const parseEngineError = (err: unknown): { code: string; message: string } => {
    const raw = err instanceof Error ? err.message : String(err ?? "");
    const jsonStart = raw.indexOf("{");
    if (jsonStart >= 0) {
      try {
        const body = JSON.parse(raw.slice(jsonStart));
        return {
          code: String(body?.code || "").trim(),
          message: String(body?.error || body?.message || "").trim(),
        };
      } catch {
        // fall through
      }
    }
    return { code: "", message: raw };
  };

  const friendlyEngineError = (err: unknown, fallback: string): string => {
    const { code, message } = parseEngineError(err);
    if (
      code === "AUTOMATION_V2_RUN_TASK_NOT_MUTABLE" ||
      code === "AUTOMATION_V2_RUN_NOT_RECOVERABLE"
    ) {
      return "Run is still active. Pause or cancel it first, then retry.";
    }
    return message || fallback;
  };

  const isRunNotMutableError = (err: unknown): boolean => {
    const { code } = parseEngineError(err);
    return (
      code === "AUTOMATION_V2_RUN_TASK_NOT_MUTABLE" || code === "AUTOMATION_V2_RUN_NOT_RECOVERABLE"
    );
  };

  const withAutoPauseRetry = async <T>(runId: string, action: () => Promise<T>): Promise<T> => {
    try {
      return await action();
    } catch (err) {
      if (!isRunNotMutableError(err) || !client?.automationsV2?.pauseRun) {
        throw err;
      }
      try {
        await client.automationsV2.pauseRun(runId, "auto-pause for retry");
      } catch {
        // ignore — engine may already be in a state pause cannot apply to
      }
      await new Promise((resolve) => setTimeout(resolve, 400));
      return await action();
    }
  };

  const runActionMutation = useMutation({
    mutationFn: async ({
      action,
      runId,
      family,
      reason,
    }: {
      action: "pause" | "resume" | "cancel";
      runId: string;
      family: "legacy" | "v2";
      reason?: string;
    }) => {
      if (family === "v2") {
        if (action === "cancel") return client.automationsV2.cancelRun(runId, reason);
        if (action === "pause") return client.automationsV2.pauseRun(runId, reason);
        return client.automationsV2.resumeRun(runId, reason);
      }
      if (action === "cancel") {
        throw new Error("Cancel is only available for workflow runs in this client.");
      }
      if (action === "pause") return client.automations.pauseRun(runId, reason);
      return client.automations.resumeRun(runId, reason);
    },
    onSuccess: async (_payload, vars) => {
      if (vars.action === "cancel") {
        toast("ok", "Run cancelled.");
      } else {
        toast("ok", "Run action applied.");
      }
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const workflowRepairMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.repairRun) {
        throw new Error("Workflow repair is not available in this client.");
      }
      return withAutoPauseRetry(runId, () =>
        client.automationsV2.repairRun(runId, {
          node_id: nodeId,
          reason: reason ?? "",
        })
      );
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Workflow continued from blocked step.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Workflow repair failed.")),
  });
  const workflowRecoverMutation = useMutation({
    mutationFn: async ({ runId, reason }: { runId: string; reason?: string }) => {
      if (!client?.automationsV2?.recoverRun) {
        throw new Error("Workflow retry is not available in this client.");
      }
      return withAutoPauseRetry(runId, () => client.automationsV2.recoverRun(runId, reason));
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Workflow run queued again.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Workflow retry failed.")),
  });
  const workflowTaskRetryMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.retryTask) {
        throw new Error("Task retry is not available in this client.");
      }
      return withAutoPauseRetry(runId, () => client.automationsV2.retryTask(runId, nodeId, reason));
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Task retried and subtree requeued.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Task retry failed.")),
  });
  const workflowTaskContinueMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.continueTask) {
        throw new Error("Task continue is not available in this client.");
      }
      return withAutoPauseRetry(runId, () =>
        client.automationsV2.continueTask(runId, nodeId, reason)
      );
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Blocked task continued with minimal reset.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Task continue failed.")),
  });
  const workflowTaskRequeueMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      reason,
    }: {
      runId: string;
      nodeId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.requeueTask) {
        throw new Error("Task requeue is not available in this client.");
      }
      return withAutoPauseRetry(runId, () =>
        client.automationsV2.requeueTask(runId, nodeId, reason)
      );
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      toast("ok", "Task requeued and subtree reset.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Task requeue failed.")),
  });
  const workflowTaskDispositionMutation = useMutation({
    mutationFn: async ({
      runId,
      nodeId,
      disposition,
      reason,
    }: {
      runId: string;
      nodeId: string;
      disposition: "unmarked" | "accepted" | "rejected" | "re_ran_strict";
      reason?: string;
    }) => {
      if (!client?.automationsV2?.setTaskDisposition) {
        throw new Error("Task disposition is not available in this client.");
      }
      return client.automationsV2.setTaskDisposition(runId, nodeId, disposition, reason);
    },
    onSuccess: async (payload: any) => {
      const runId = String(
        payload?.run?.run_id || payload?.run?.runId || selectedRunId || ""
      ).trim();
      const disposition = String(payload?.disposition || "");
      const changed = payload?.changed === true;
      toast(
        "ok",
        changed
          ? `Marked artifact as ${disposition.replace(/_/g, " ")}.`
          : `Already marked ${disposition.replace(/_/g, " ")}.`
      );
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (runId) {
        onSelectRunId(runId);
      }
    },
    onError: (error) => toast("err", friendlyEngineError(error, "Disposition update failed.")),
  });
  const backlogTaskClaimMutation = useMutation({
    mutationFn: async ({
      runId,
      taskId,
      agentId,
      reason,
    }: {
      runId: string;
      taskId: string;
      agentId?: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.claimBacklogTask) {
        throw new Error("Backlog task claim is not available in this client.");
      }
      return client.automationsV2.claimBacklogTask(runId, taskId, {
        agent_id: agentId,
        reason,
      });
    },
    onSuccess: async () => {
      toast("ok", "Backlog task claimed.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (selectedRunId) {
        onSelectRunId(selectedRunId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  const backlogTaskRequeueMutation = useMutation({
    mutationFn: async ({
      runId,
      taskId,
      reason,
    }: {
      runId: string;
      taskId: string;
      reason?: string;
    }) => {
      if (!client?.automationsV2?.requeueBacklogTask) {
        throw new Error("Backlog task requeue is not available in this client.");
      }
      return client.automationsV2.requeueBacklogTask(runId, taskId, reason);
    },
    onSuccess: async () => {
      toast("ok", "Backlog task requeued.");
      await queryClient.invalidateQueries({ queryKey: ["automations"] });
      if (selectedRunId) {
        onSelectRunId(selectedRunId);
        onOpenRunningView();
      }
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });
  useEffect(() => {
    runActionMutation.reset();
    workflowRepairMutation.reset();
    workflowRecoverMutation.reset();
    workflowTaskRetryMutation.reset();
    workflowTaskContinueMutation.reset();
    workflowTaskRequeueMutation.reset();
    workflowTaskDispositionMutation.reset();
    backlogTaskClaimMutation.reset();
    backlogTaskRequeueMutation.reset();
  }, [
    backlogTaskClaimMutation,
    backlogTaskRequeueMutation,
    runActionMutation,
    selectedBoardTaskId,
    selectedRunId,
    workflowRecoverMutation,
    workflowRepairMutation,
    workflowTaskContinueMutation,
    workflowTaskDispositionMutation,
    workflowTaskRequeueMutation,
    workflowTaskRetryMutation,
  ]);

  return {
    runNowMutation,
    runNowV2Mutation,
    runActionMutation,
    workflowRepairMutation,
    workflowRecoverMutation,
    workflowTaskRetryMutation,
    workflowTaskContinueMutation,
    workflowTaskRequeueMutation,
    workflowTaskDispositionMutation,
    backlogTaskClaimMutation,
    backlogTaskRequeueMutation,
  };
}
