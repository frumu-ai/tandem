import {
  SCHEDULE_WEEKDAY_OPTIONS,
  describeScheduleValue,
  friendlyScheduleToValue,
  scheduleValueToFriendly,
  setFriendlyScheduleMode,
  type FriendlyScheduleMode,
  type ScheduleValue,
} from "@/components/agent-automation/scheduleBuilder";

type ScheduleBuilderProps = {
  value: ScheduleValue;
  onChange: (value: ScheduleValue) => void;
};

const SCHEDULE_MODE_OPTIONS: Array<{
  id: FriendlyScheduleMode;
  label: string;
  desc: string;
}> = [
  { id: "manual", label: "Manual", desc: "Only run when you trigger it." },
  { id: "daily", label: "Every day", desc: "Run once a day at a specific time." },
  { id: "weekdays", label: "Weekdays", desc: "Run Monday through Friday." },
  { id: "selected_days", label: "Selected days", desc: "Choose specific days of the week." },
  { id: "monthly", label: "Monthly", desc: "Run on a day of the month." },
  { id: "interval", label: "Repeating", desc: "Run every N minutes or hours." },
  { id: "advanced", label: "Advanced cron", desc: "Edit the raw cron expression directly." },
];

export function ScheduleBuilder({ value, onChange }: ScheduleBuilderProps) {
  const friendly = scheduleValueToFriendly(value);
  const summary = describeScheduleValue(value);

  const commit = (nextFriendly: ReturnType<typeof scheduleValueToFriendly>) => {
    onChange(friendlyScheduleToValue(nextFriendly));
  };

  return (
    <div className="space-y-3">
      <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
        {SCHEDULE_MODE_OPTIONS.map((option) => (
          <button
            key={option.id}
            type="button"
            className={`rounded-lg border px-3 py-3 text-left transition-all ${
              friendly.mode === option.id
                ? "border-primary bg-primary/10 text-text"
                : "border-border bg-surface-elevated/40 text-text"
            }`}
            onClick={() => commit(setFriendlyScheduleMode(friendly, option.id))}
          >
            <div className="text-sm font-medium">{option.label}</div>
            <div className="mt-1 text-xs text-text-muted">{option.desc}</div>
          </button>
        ))}
      </div>

      {friendly.mode === "daily" ||
      friendly.mode === "weekdays" ||
      friendly.mode === "selected_days" ? (
        <div className="grid gap-3 rounded-lg border border-border bg-surface-elevated/30 p-3">
          <div className="grid gap-1">
            <label className="text-xs font-medium uppercase tracking-wide text-text-subtle">
              Run Time
            </label>
            <input
              type="time"
              className="h-10 rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
              value={friendly.time}
              onChange={(event) =>
                commit({
                  ...friendly,
                  time: event.target.value || "09:00",
                })
              }
            />
          </div>
          {friendly.mode === "selected_days" ? (
            <div className="grid gap-1">
              <label className="text-xs font-medium uppercase tracking-wide text-text-subtle">
                Days Of Week
              </label>
              <div className="flex flex-wrap gap-2">
                {SCHEDULE_WEEKDAY_OPTIONS.map((option) => {
                  const selected = friendly.weekdays.includes(option.value);
                  return (
                    <button
                      key={option.value}
                      type="button"
                      className={`rounded-lg border px-3 py-2 text-xs font-medium ${
                        selected
                          ? "border-primary bg-primary/10 text-text"
                          : "border-border bg-surface text-text-muted"
                      }`}
                      onClick={() => {
                        const weekdays = selected
                          ? friendly.weekdays.filter((value) => value !== option.value)
                          : [...friendly.weekdays, option.value].sort((a, b) => {
                              const aIndex = a === 0 ? 7 : a;
                              const bIndex = b === 0 ? 7 : b;
                              return aIndex - bIndex;
                            });
                        commit({
                          ...friendly,
                          weekdays: weekdays.length ? weekdays : [1],
                        });
                      }}
                    >
                      {option.label}
                    </button>
                  );
                })}
              </div>
            </div>
          ) : null}
        </div>
      ) : null}

      {friendly.mode === "monthly" ? (
        <div className="grid gap-3 rounded-lg border border-border bg-surface-elevated/30 p-3 sm:grid-cols-2">
          <label className="grid gap-1 text-sm font-medium text-text">
            <span className="text-xs font-medium uppercase tracking-wide text-text-subtle">
              Day Of Month
            </span>
            <select
              className="h-10 rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
              value={String(friendly.monthlyDay)}
              onChange={(event) =>
                commit({
                  ...friendly,
                  monthlyDay: Number.parseInt(
                    event.target.value || String(friendly.monthlyDay),
                    10
                  ),
                })
              }
            >
              {Array.from({ length: 31 }, (_, index) => index + 1).map((day) => (
                <option key={day} value={day}>
                  {day}
                </option>
              ))}
            </select>
          </label>
          <label className="grid gap-1 text-sm font-medium text-text">
            <span className="text-xs font-medium uppercase tracking-wide text-text-subtle">
              Run Time
            </span>
            <input
              type="time"
              className="h-10 rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
              value={friendly.time}
              onChange={(event) =>
                commit({
                  ...friendly,
                  time: event.target.value || "09:00",
                })
              }
            />
          </label>
        </div>
      ) : null}

      {friendly.mode === "interval" ? (
        <div className="grid gap-3 rounded-lg border border-border bg-surface-elevated/30 p-3 sm:grid-cols-[minmax(0,1fr)_180px]">
          <label className="grid gap-1 text-sm font-medium text-text">
            <span className="text-xs font-medium uppercase tracking-wide text-text-subtle">
              Repeat Every
            </span>
            <input
              type="number"
              min="1"
              className="h-10 rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
              value={friendly.intervalValue}
              onChange={(event) =>
                commit({
                  ...friendly,
                  intervalValue: event.target.value || "1",
                })
              }
            />
          </label>
          <label className="grid gap-1 text-sm font-medium text-text">
            <span className="text-xs font-medium uppercase tracking-wide text-text-subtle">
              Unit
            </span>
            <select
              className="h-10 rounded-lg border border-border bg-surface px-3 text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
              value={friendly.intervalUnit}
              onChange={(event) =>
                commit({
                  ...friendly,
                  intervalUnit: event.target.value as "hours" | "minutes",
                })
              }
            >
              <option value="hours">hours</option>
              <option value="minutes">minutes</option>
            </select>
          </label>
        </div>
      ) : null}

      {friendly.mode === "advanced" ? (
        <div className="grid gap-1 rounded-lg border border-border bg-surface-elevated/30 p-3">
          <label className="text-xs font-medium uppercase tracking-wide text-text-subtle">
            Cron Expression
          </label>
          <input
            className="h-10 rounded-lg border border-border bg-surface px-3 font-mono text-sm text-text outline-none focus:border-primary focus:ring-1 focus:ring-primary"
            placeholder="e.g. 30 8 * * 1-5"
            value={friendly.rawCron}
            onChange={(event) =>
              commit({
                ...friendly,
                rawCron: event.target.value,
              })
            }
          />
          <div className="text-xs text-text-subtle">
            Advanced mode keeps the raw cron expression for schedules that do not fit the guided
            options above.
          </div>
        </div>
      ) : null}

      <div className="rounded-lg border border-primary/20 bg-primary/10 px-3 py-2 text-sm text-text">
        Schedule preview: <span className="font-medium">{summary}</span>
      </div>
    </div>
  );
}
