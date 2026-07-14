import { useCallback, useState } from "react";
import type { TandemClient } from "@frumu/tandem-client";
import type { ChatWorkflowArtifact } from "./workflowArtifact";
import type { WorkflowArtifactAction } from "./WorkflowArtifactCard";

type UseWorkflowArtifactActionsOptions = {
  client: TandemClient;
  artifact: ChatWorkflowArtifact | null;
  refresh: () => Promise<unknown>;
  sendPrompt: (promptOverride?: string) => Promise<void>;
  sending: boolean;
  prepareRevision: (prompt: string) => void;
  toast: (kind: "ok" | "info" | "warn" | "err", text: string) => void;
};

function errorText(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function openPlanner(sessionId: string, resourceUrl = "") {
  const hashIndex = resourceUrl.indexOf("#");
  window.location.hash =
    hashIndex >= 0
      ? resourceUrl.slice(hashIndex + 1)
      : `#/planner?session_id=${encodeURIComponent(sessionId)}`;
}

export function useWorkflowArtifactActions({
  client,
  artifact,
  refresh,
  sendPrompt,
  sending,
  prepareRevision,
  toast,
}: UseWorkflowArtifactActionsOptions) {
  const [actionBusy, setActionBusy] = useState<WorkflowArtifactAction | "">("");

  const handleAction = useCallback(
    async (action: WorkflowArtifactAction) => {
      if (!artifact || actionBusy || sending) return;
      if (action === "open") {
        openPlanner(artifact.sessionId, artifact.plannerUrl);
        return;
      }
      if (action === "revise") {
        prepareRevision(`Revise workflow "${artifact.title}" (revision ${artifact.revision}): `);
        return;
      }

      setActionBusy(action);
      try {
        if (action === "duplicate") {
          const response = await client.workflowPlannerSessions.duplicate(artifact.sessionId, {
            title: `${artifact.title} copy`,
          });
          toast("ok", "Workflow draft duplicated.");
          openPlanner(response.session.session_id);
          return;
        }

        const command =
          action === "validate"
            ? `Validate workflow planner session ${artifact.sessionId} at revision ${artifact.revision}. Use the first-party workflow validation tool and report authoritative blockers, warnings, required connections, and approvals.`
            : `Create a disabled Automation V2 draft from workflow planner session ${artifact.sessionId} at revision ${artifact.revision}. Validate it first and do not publish or enable it.`;
        await sendPrompt(command);
        await refresh();
      } catch (error) {
        toast("err", errorText(error));
      } finally {
        setActionBusy("");
      }
    },
    [
      actionBusy,
      artifact,
      client.workflowPlannerSessions,
      prepareRevision,
      refresh,
      sendPrompt,
      sending,
      toast,
    ]
  );

  return { actionBusy, handleAction };
}
