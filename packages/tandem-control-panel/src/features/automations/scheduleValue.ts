export type ScheduleKind = "manual" | "cron" | "interval";

export type ScheduleValue = {
  scheduleKind: ScheduleKind;
  cronExpression: string;
  intervalSeconds: string;
};

export type ScheduleDescriptionOptions = {
  timezone?: string;
};

export type FriendlyScheduleMode =
  | "manual"
  | "daily"
  | "weekdays"
  | "selected_days"
  | "monthly"
  | "interval"
  | "advanced";

export type FriendlySchedule = {
  mode: FriendlyScheduleMode;
  time: string;
  weekdays: number[];
  monthlyDay: number;
  intervalValue: string;
  intervalUnit: "hours" | "minutes";
  rawCron: string;
};

export const SCHEDULE_WEEKDAY_OPTIONS = [
  { value: 1, label: "Mon" },
  { value: 2, label: "Tue" },
  { value: 3, label: "Wed" },
  { value: 4, label: "Thu" },
  { value: 5, label: "Fri" },
  { value: 6, label: "Sat" },
  { value: 0, label: "Sun" },
];

const DEFAULT_TIME = "09:00";

function pad2(value: number) {
  return String(value).padStart(2, "0");
}

function parseCronFields(expression: string) {
  const fields = String(expression || "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (fields.length !== 5) return null;
  return {
    minute: fields[0],
    hour: fields[1],
    dayOfMonth: fields[2],
    month: fields[3],
    dayOfWeek: fields[4],
  };
}

function parsePositiveInt(raw: string) {
  const value = Number.parseInt(String(raw || ""), 10);
  return Number.isFinite(value) ? value : null;
}

function toTime(hour: string, minute: string) {
  const hourValue = parsePositiveInt(hour);
  const minuteValue = parsePositiveInt(minute);
  if (hourValue === null || minuteValue === null) return DEFAULT_TIME;
  if (hourValue < 0 || hourValue > 23 || minuteValue < 0 || minuteValue > 59) return DEFAULT_TIME;
  return `${pad2(hourValue)}:${pad2(minuteValue)}`;
}

function weekdayLabel(value: number) {
  return SCHEDULE_WEEKDAY_OPTIONS.find((option) => option.value === value)?.label || `Day ${value}`;
}

function expandDayOfWeekField(field: string) {
  const value = String(field || "").trim();
  if (!value || value === "*") return null;
  if (value === "1-5") return [1, 2, 3, 4, 5];
  const out = new Set<number>();
  for (const part of value.split(",")) {
    const trimmed = part.trim();
    if (!trimmed) continue;
    if (trimmed.includes("-")) {
      const [startText, endText] = trimmed.split("-");
      const start = parsePositiveInt(startText);
      const end = parsePositiveInt(endText);
      if (start === null || end === null || start > end) return null;
      for (let current = start; current <= end; current += 1) {
        if (current < 0 || current > 7) return null;
        out.add(current === 7 ? 0 : current);
      }
      continue;
    }
    const parsed = parsePositiveInt(trimmed);
    if (parsed === null || parsed < 0 || parsed > 7) return null;
    out.add(parsed === 7 ? 0 : parsed);
  }
  return Array.from(out.values()).sort((a, b) => {
    const aIndex = a === 0 ? 7 : a;
    const bIndex = b === 0 ? 7 : b;
    return aIndex - bIndex;
  });
}

function format12Hour(time: string) {
  const [hourText, minuteText] = String(time || DEFAULT_TIME).split(":");
  const hour = Number.parseInt(hourText || "9", 10) || 0;
  const minute = Number.parseInt(minuteText || "0", 10) || 0;
  const meridiem = hour >= 12 ? "PM" : "AM";
  const displayHour = hour % 12 || 12;
  return `${displayHour}:${pad2(minute)} ${meridiem}`;
}

function listWeekdays(weekdays: number[]) {
  const labels = weekdays.map((value) => weekdayLabel(value));
  if (labels.length <= 1) return labels[0] || "";
  if (labels.length === 2) return `${labels[0]} and ${labels[1]}`;
  return `${labels.slice(0, -1).join(", ")}, and ${labels[labels.length - 1]}`;
}

function cronForFriendlySchedule(schedule: FriendlySchedule) {
  const [hour, minute] = schedule.time.split(":");
  const minuteValue = pad2(Math.max(0, Math.min(59, parsePositiveInt(minute) ?? 0)));
  const hourValue = String(Math.max(0, Math.min(23, parsePositiveInt(hour) ?? 9)));
  if (schedule.mode === "daily") return `${minuteValue} ${hourValue} * * *`;
  if (schedule.mode === "weekdays") return `${minuteValue} ${hourValue} * * 1-5`;
  if (schedule.mode === "selected_days") {
    const weekdays = schedule.weekdays.length ? schedule.weekdays : [1];
    return `${minuteValue} ${hourValue} * * ${weekdays.join(",")}`;
  }
  if (schedule.mode === "monthly") {
    const day = Math.max(
      1,
      Math.min(31, Number.parseInt(String(schedule.monthlyDay || 1), 10) || 1)
    );
    return `${minuteValue} ${hourValue} ${day} * *`;
  }
  return schedule.rawCron;
}

function intervalParts(intervalSeconds: string) {
  const seconds = Math.max(60, Number.parseInt(String(intervalSeconds || "3600"), 10) || 3600);
  if (seconds % 3600 === 0) {
    return {
      intervalValue: String(Math.max(1, seconds / 3600)),
      intervalUnit: "hours" as const,
    };
  }
  return {
    intervalValue: String(Math.max(1, Math.round(seconds / 60))),
    intervalUnit: "minutes" as const,
  };
}

export function scheduleValueToFriendly(value: ScheduleValue): FriendlySchedule {
  if (value.scheduleKind === "manual") {
    return {
      mode: "manual",
      time: DEFAULT_TIME,
      weekdays: [1, 2, 3, 4, 5],
      monthlyDay: 1,
      intervalValue: "1",
      intervalUnit: "hours",
      rawCron: "",
    };
  }
  if (value.scheduleKind === "interval") {
    const interval = intervalParts(value.intervalSeconds);
    return {
      mode: "interval",
      time: DEFAULT_TIME,
      weekdays: [1, 2, 3, 4, 5],
      monthlyDay: 1,
      intervalValue: interval.intervalValue,
      intervalUnit: interval.intervalUnit,
      rawCron: "",
    };
  }
  const rawCron = String(value.cronExpression || "").trim();
  const fields = parseCronFields(rawCron);
  if (!fields) {
    return {
      mode: "advanced",
      time: DEFAULT_TIME,
      weekdays: [1, 2, 3, 4, 5],
      monthlyDay: 1,
      intervalValue: "1",
      intervalUnit: "hours",
      rawCron,
    };
  }
  const time = toTime(fields.hour, fields.minute);
  const monthIsWildcard = fields.month === "*";
  if (
    /^\d+$/.test(fields.minute) &&
    /^\d+$/.test(fields.hour) &&
    monthIsWildcard &&
    fields.dayOfMonth === "*" &&
    fields.dayOfWeek === "*"
  ) {
    return {
      mode: "daily",
      time,
      weekdays: [1, 2, 3, 4, 5],
      monthlyDay: 1,
      intervalValue: "1",
      intervalUnit: "hours",
      rawCron,
    };
  }
  if (
    /^\d+$/.test(fields.minute) &&
    /^\d+$/.test(fields.hour) &&
    monthIsWildcard &&
    fields.dayOfMonth === "*" &&
    fields.dayOfWeek === "1-5"
  ) {
    return {
      mode: "weekdays",
      time,
      weekdays: [1, 2, 3, 4, 5],
      monthlyDay: 1,
      intervalValue: "1",
      intervalUnit: "hours",
      rawCron,
    };
  }
  const selectedDays = expandDayOfWeekField(fields.dayOfWeek);
  if (
    /^\d+$/.test(fields.minute) &&
    /^\d+$/.test(fields.hour) &&
    monthIsWildcard &&
    fields.dayOfMonth === "*" &&
    selectedDays?.length
  ) {
    return {
      mode: "selected_days",
      time,
      weekdays: selectedDays,
      monthlyDay: 1,
      intervalValue: "1",
      intervalUnit: "hours",
      rawCron,
    };
  }
  if (
    /^\d+$/.test(fields.minute) &&
    /^\d+$/.test(fields.hour) &&
    monthIsWildcard &&
    /^\d+$/.test(fields.dayOfMonth) &&
    fields.dayOfWeek === "*"
  ) {
    return {
      mode: "monthly",
      time,
      weekdays: [1, 2, 3, 4, 5],
      monthlyDay: Math.max(1, Math.min(31, Number.parseInt(fields.dayOfMonth, 10) || 1)),
      intervalValue: "1",
      intervalUnit: "hours",
      rawCron,
    };
  }
  return {
    mode: "advanced",
    time,
    weekdays: [1, 2, 3, 4, 5],
    monthlyDay: 1,
    intervalValue: "1",
    intervalUnit: "hours",
    rawCron,
  };
}

export function friendlyScheduleToValue(schedule: FriendlySchedule): ScheduleValue {
  if (schedule.mode === "manual") {
    return {
      scheduleKind: "manual",
      cronExpression: "",
      intervalSeconds: "3600",
    };
  }
  if (schedule.mode === "interval") {
    const value = Math.max(1, Number.parseInt(String(schedule.intervalValue || "1"), 10) || 1);
    const seconds = schedule.intervalUnit === "minutes" ? value * 60 : value * 3600;
    return {
      scheduleKind: "interval",
      cronExpression: "",
      intervalSeconds: String(seconds),
    };
  }
  const cronExpression =
    schedule.mode === "advanced"
      ? String(schedule.rawCron || "").trim()
      : cronForFriendlySchedule(schedule);
  return {
    scheduleKind: "cron",
    cronExpression,
    intervalSeconds: "3600",
  };
}

export function setFriendlyScheduleMode(
  current: FriendlySchedule,
  mode: FriendlyScheduleMode
): FriendlySchedule {
  if (mode === current.mode) return current;
  if (mode === "manual") return { ...current, mode };
  if (mode === "interval") {
    return {
      ...current,
      mode,
      intervalValue: current.intervalValue || "1",
      intervalUnit: current.intervalUnit || "hours",
    };
  }
  if (mode === "advanced") {
    const rawCron =
      current.mode === "advanced"
        ? current.rawCron
        : current.mode === "manual" || current.mode === "interval"
          ? "0 9 * * *"
          : cronForFriendlySchedule(current);
    return {
      ...current,
      mode,
      rawCron,
    };
  }
  if (mode === "weekdays") return { ...current, mode, weekdays: [1, 2, 3, 4, 5] };
  if (mode === "selected_days") {
    return {
      ...current,
      mode,
      weekdays: current.weekdays.length ? current.weekdays : [1],
    };
  }
  if (mode === "monthly") return { ...current, mode, monthlyDay: current.monthlyDay || 1 };
  return { ...current, mode };
}

function scheduleTimezoneLabel(timezone: string | undefined) {
  return String(timezone || "").trim() || "local time";
}

export function describeScheduleValue(
  value: ScheduleValue,
  options: ScheduleDescriptionOptions = {}
) {
  const friendly = scheduleValueToFriendly(value);
  const timezone = scheduleTimezoneLabel(options.timezone);
  if (friendly.mode === "manual") return "Manual only";
  if (friendly.mode === "interval") {
    const amount = Math.max(1, Number.parseInt(String(friendly.intervalValue || "1"), 10) || 1);
    return `Every ${amount} ${friendly.intervalUnit === "hours" ? (amount === 1 ? "hour" : "hours") : amount === 1 ? "minute" : "minutes"}`;
  }
  if (friendly.mode === "daily") return `Every day at ${format12Hour(friendly.time)} ${timezone}`;
  if (friendly.mode === "weekdays")
    return `Every weekday at ${format12Hour(friendly.time)} ${timezone}`;
  if (friendly.mode === "selected_days") {
    return `${listWeekdays(friendly.weekdays)} at ${format12Hour(friendly.time)} ${timezone}`;
  }
  if (friendly.mode === "monthly") {
    return `Day ${friendly.monthlyDay} of every month at ${format12Hour(friendly.time)} ${timezone}`;
  }
  return friendly.rawCron || "Custom cron";
}
