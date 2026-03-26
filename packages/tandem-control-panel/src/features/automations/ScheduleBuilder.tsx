import {
  SCHEDULE_WEEKDAY_OPTIONS,
  describeScheduleValue,
  friendlyScheduleToValue,
  scheduleValueToFriendly,
  setFriendlyScheduleMode,
  type FriendlyScheduleMode,
  type ScheduleValue,
} from "./scheduleBuilder";

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
    <div className="grid gap-3">
      <div className="grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
        {SCHEDULE_MODE_OPTIONS.map((option) => (
          <button
            key={option.id}
            type="button"
            className={`tcp-list-item flex flex-col items-start gap-1 text-left transition-all ${
              friendly.mode === option.id ? "border-amber-400/60 bg-amber-400/10" : ""
            }`}
            onClick={() => commit(setFriendlyScheduleMode(friendly, option.id))}
          >
            <div className="font-medium">{option.label}</div>
            <div className="tcp-subtle text-xs">{option.desc}</div>
          </button>
        ))}
      </div>

      {friendly.mode === "daily" ||
      friendly.mode === "weekdays" ||
      friendly.mode === "selected_days" ? (
        <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Run time</label>
            <input
              type="time"
              className="tcp-input"
              value={friendly.time}
              onInput={(e) =>
                commit({
                  ...friendly,
                  time: (e.target as HTMLInputElement).value || "09:00",
                })
              }
            />
          </div>
          {friendly.mode === "selected_days" ? (
            <div className="grid gap-1">
              <label className="text-xs text-slate-400">Days of week</label>
              <div className="flex flex-wrap gap-2">
                {SCHEDULE_WEEKDAY_OPTIONS.map((option) => {
                  const selected = friendly.weekdays.includes(option.value);
                  return (
                    <button
                      key={option.value}
                      type="button"
                      className={`tcp-btn h-8 px-3 text-xs ${
                        selected ? "border-amber-400/60 bg-amber-400/10 text-amber-300" : ""
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
        <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3 sm:grid-cols-2">
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Day of month</label>
            <select
              className="tcp-select"
              value={String(friendly.monthlyDay)}
              onInput={(e) =>
                commit({
                  ...friendly,
                  monthlyDay: Number.parseInt(
                    (e.target as HTMLSelectElement).value || String(friendly.monthlyDay),
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
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Run time</label>
            <input
              type="time"
              className="tcp-input"
              value={friendly.time}
              onInput={(e) =>
                commit({
                  ...friendly,
                  time: (e.target as HTMLInputElement).value || "09:00",
                })
              }
            />
          </div>
        </div>
      ) : null}

      {friendly.mode === "interval" ? (
        <div className="grid gap-3 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3 sm:grid-cols-[minmax(0,1fr)_180px]">
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Repeat every</label>
            <input
              type="number"
              min="1"
              className="tcp-input"
              value={friendly.intervalValue}
              onInput={(e) =>
                commit({
                  ...friendly,
                  intervalValue: (e.target as HTMLInputElement).value || "1",
                })
              }
            />
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Unit</label>
            <select
              className="tcp-select"
              value={friendly.intervalUnit}
              onInput={(e) =>
                commit({
                  ...friendly,
                  intervalUnit: (e.target as HTMLSelectElement).value as "hours" | "minutes",
                })
              }
            >
              <option value="hours">hours</option>
              <option value="minutes">minutes</option>
            </select>
          </div>
        </div>
      ) : null}

      {friendly.mode === "advanced" ? (
        <div className="grid gap-1 rounded-xl border border-slate-700/50 bg-slate-900/30 p-3">
          <label className="text-xs text-slate-400">Cron expression</label>
          <input
            className="tcp-input font-mono"
            placeholder="e.g. 30 8 * * 1-5"
            value={friendly.rawCron}
            onInput={(e) =>
              commit({
                ...friendly,
                rawCron: (e.target as HTMLInputElement).value,
              })
            }
          />
          <div className="text-xs text-slate-500">
            Advanced mode keeps the raw cron expression for schedules that don&apos;t fit the guided
            options above.
          </div>
        </div>
      ) : null}

      <div className="rounded-xl border border-amber-400/20 bg-amber-400/5 px-3 py-2 text-xs text-amber-100">
        {summary}
      </div>
    </div>
  );
}
