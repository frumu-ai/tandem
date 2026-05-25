import { Button } from "@/components/ui";
import type { AutomationV2RunRecord } from "@/lib/tauri";
import {
  canCancelRun,
  canPauseRun,
  canRecoverRun,
  canResumeRun,
  runAwaitingGate,
} from "./coderRunUtils";

type CoderRunActionToolbarProps = {
  run: AutomationV2RunRecord;
  busyKey: string | null;
  onRefresh: () => void;
  onRunAction: (runId: string, action: "pause" | "resume" | "cancel" | "recover") => void;
  onGateDecision: (runId: string, decision: "approve" | "rework" | "cancel") => void;
};

export function CoderRunActionToolbar({
  run,
  busyKey,
  onRefresh,
  onRunAction,
  onGateDecision,
}: CoderRunActionToolbarProps) {
  const awaitingGate = runAwaitingGate(run);
  return (
    <div className="flex flex-wrap gap-2">
      <Button
        size="sm"
        variant="secondary"
        onClick={onRefresh}
        disabled={busyKey === `inspect:${run.run_id}`}
      >
        Refresh Detail
      </Button>
      {canPauseRun(run) ? (
        <Button
          size="sm"
          variant="secondary"
          onClick={() => onRunAction(run.run_id, "pause")}
          disabled={busyKey === `pause:${run.run_id}`}
        >
          Pause
        </Button>
      ) : null}
      {canResumeRun(run) ? (
        <Button
          size="sm"
          variant="secondary"
          onClick={() => onRunAction(run.run_id, "resume")}
          disabled={busyKey === `resume:${run.run_id}`}
        >
          Resume
        </Button>
      ) : null}
      {canRecoverRun(run) ? (
        <Button
          size="sm"
          variant="secondary"
          onClick={() => onRunAction(run.run_id, "recover")}
          disabled={busyKey === `recover:${run.run_id}`}
        >
          Recover
        </Button>
      ) : null}
      {canCancelRun(run) ? (
        <Button
          size="sm"
          variant="danger"
          onClick={() => onRunAction(run.run_id, "cancel")}
          disabled={busyKey === `cancel:${run.run_id}`}
        >
          Cancel
        </Button>
      ) : null}
      {awaitingGate ? (
        <>
          <Button
            size="sm"
            variant="primary"
            onClick={() => onGateDecision(run.run_id, "approve")}
            disabled={busyKey === `gate:approve:${run.run_id}`}
          >
            Approve
          </Button>
          <Button
            size="sm"
            variant="secondary"
            onClick={() => onGateDecision(run.run_id, "rework")}
            disabled={busyKey === `gate:rework:${run.run_id}`}
          >
            Rework
          </Button>
        </>
      ) : null}
    </div>
  );
}
