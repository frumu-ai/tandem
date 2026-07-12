import { Icon } from "../../ui/Icon";
import type { GoalOperationMode, GoalTimelineEntry } from "./types";

function formatTime(timestamp: number) {
  if (!timestamp) return "Time unavailable";
  return new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  }).format(new Date(timestamp));
}

function eventLabel(entry: GoalTimelineEntry) {
  const payload = entry.event.payload && typeof entry.event.payload === "object"
    ? entry.event.payload as Record<string, unknown>
    : {};
  return String(payload.summary || entry.event.event_type || "Runtime event").replaceAll("_", " ");
}

export function GoalReplayTimeline({
  mode,
  timeline,
  replayIndex,
  onModeChange,
  onScrub,
}: {
  mode: GoalOperationMode;
  timeline: GoalTimelineEntry[];
  replayIndex: number;
  onModeChange: (mode: GoalOperationMode) => void;
  onScrub: (index: number) => void;
}) {
  const activeIndex = mode === "live" ? Math.max(0, timeline.length - 1) : replayIndex;
  const active = timeline[activeIndex];
  return (
    <section className="goal-ops-timeline" aria-label="Goal timeline and replay">
      <div className="goal-ops-timeline-toolbar">
        <div className="goal-ops-segmented" role="group" aria-label="Timeline mode">
          <button
            type="button"
            className={mode === "live" ? "active" : ""}
            aria-pressed={mode === "live"}
            onClick={() => onModeChange("live")}
          >
            <Icon name="radio" size={14} /> Live
          </button>
          <button
            type="button"
            className={mode === "replay" ? "active" : ""}
            aria-pressed={mode === "replay"}
            onClick={() => onModeChange("replay")}
          >
            <Icon name="history" size={14} /> Replay
          </button>
        </div>
        <label className="goal-ops-scrubber">
          <span>Replay position</span>
          <input
            type="range"
            min={0}
            max={Math.max(0, timeline.length - 1)}
            value={activeIndex}
            disabled={mode !== "replay" || timeline.length < 2}
            onChange={(event) => onScrub(Number(event.currentTarget.value))}
          />
          <output>{timeline.length ? `${activeIndex + 1} of ${timeline.length}` : "No events"}</output>
        </label>
      </div>
      <div className="goal-ops-timeline-list" role="list" aria-label="Bounded goal events">
        {timeline.length ? timeline.map((entry, index) => (
          <button
            type="button"
            role="listitem"
            key={entry.event.event_id || `${entry.event.event_type}-${entry.event.occurred_at_ms}-${index}`}
            className={index === activeIndex ? "active" : ""}
            aria-current={index === activeIndex ? "step" : undefined}
            onClick={() => {
              onModeChange("replay");
              onScrub(index);
            }}
          >
            <span className="goal-ops-event-dot" aria-hidden="true" />
            <span className="goal-ops-event-copy">
              <strong>{eventLabel(entry)}</strong>
              <span>{formatTime(entry.event.occurred_at_ms)}</span>
            </span>
          </button>
        )) : <p className="goal-ops-muted">No runtime events have been recorded.</p>}
      </div>
      {active ? <div className="goal-ops-event-current" aria-live="polite">Viewing {eventLabel(active)} at {formatTime(active.event.occurred_at_ms)}</div> : null}
    </section>
  );
}
