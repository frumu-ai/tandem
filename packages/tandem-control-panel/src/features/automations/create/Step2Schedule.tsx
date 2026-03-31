import { ScheduleBuilder } from "../ScheduleBuilder";

type SchedulePreset = {
  label: string;
  desc: string;
  icon: string;
  cron: string;
  intervalSeconds?: number;
};

type ScheduleValue = {
  scheduleKind: "manual" | "cron" | "interval";
  cronExpression: string;
  intervalSeconds: string;
};

type Step2ScheduleProps = {
  selected: string;
  presets: SchedulePreset[];
  onSelect: (preset: SchedulePreset) => void;
  scheduleValue: ScheduleValue;
  onScheduleChange: (value: ScheduleValue) => void;
};

export function Step2Schedule({
  selected,
  presets,
  onSelect,
  scheduleValue,
  onScheduleChange,
}: Step2ScheduleProps) {
  return (
    <div className="grid gap-3">
      <p className="text-sm text-slate-400">When should this automation run?</p>
      <div className="grid gap-2 sm:grid-cols-2">
        {presets.map((preset) => (
          <button
            key={preset.label}
            onClick={() => onSelect(preset)}
            className={`tcp-list-item flex flex-col items-start gap-1 text-left transition-all ${
              selected === preset.label ? "border-amber-400/60 bg-amber-400/10" : ""
            }`}
          >
            <div className="flex items-center gap-2 font-medium">
              <span>{preset.icon}</span>
              <span>{preset.label}</span>
            </div>
            <span className="tcp-subtle text-xs">{preset.desc}</span>
            {preset.cron ? (
              <code className="rounded bg-slate-800/60 px-1.5 py-0.5 text-xs text-slate-400">
                {preset.cron}
              </code>
            ) : null}
          </button>
        ))}
      </div>
      <ScheduleBuilder value={scheduleValue} onChange={onScheduleChange} />
    </div>
  );
}
