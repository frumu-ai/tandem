import type { AutomationV2Spec } from "@/lib/tauri";
import { describeScheduleValue } from "@/components/agent-automation/scheduleBuilder";

export const CALENDAR_DISPLAY_DURATION_MS = 30 * 60 * 1000;
const CALENDAR_SLOT_MS = 60 * 1000;

export type CalendarRange = {
  startMs: number;
  endMs: number;
};

export type AutomationCalendarEvent = {
  id: string;
  title: string;
  start: Date;
  end: Date;
  allDay: false;
  editable: boolean;
  startEditable: boolean;
  durationEditable: false;
  extendedProps: {
    automation: AutomationV2Spec;
    automationId: string;
    scheduleLabel: string;
    cronExpression: string;
    status: string;
  };
};

export function formatAutomationV2ScheduleLabel(schedule: AutomationV2Spec["schedule"]) {
  const type = String(schedule?.type || "")
    .trim()
    .toLowerCase();
  if (type === "cron") {
    return describeScheduleValue({
      scheduleKind: "cron",
      cronExpression: String(schedule?.cron_expression || ""),
      intervalSeconds: "3600",
    });
  }
  if (type === "interval") {
    const seconds = Number(schedule?.interval_seconds || 0);
    if (!Number.isFinite(seconds) || seconds <= 0) return "Repeating interval";
    return describeScheduleValue({
      scheduleKind: "interval",
      cronExpression: "",
      intervalSeconds: String(seconds),
    });
  }
  return "Manual only";
}

function getAutomationCalendarTitle(automation: AutomationV2Spec) {
  return String(
    automation?.name || automation?.description || automation?.automation_id || "Automation"
  ).trim();
}

function splitCronField(field: string) {
  return String(field || "")
    .trim()
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean);
}

function matchesCronAtom(atom: string, value: number, min: number, max: number) {
  const trimmed = String(atom || "").trim();
  if (!trimmed || trimmed === "*") return true;
  const stepParts = trimmed.split("/");
  const base = stepParts[0] || "*";
  const step = stepParts[1] ? Number.parseInt(stepParts[1], 10) : 1;
  const normalizedStep = Number.isFinite(step) && step > 0 ? step : 1;
  const rangeParts = base.split("-");
  let start = min;
  let end = max;
  if (base !== "*") {
    if (rangeParts.length === 2) {
      start = Number.parseInt(rangeParts[0], 10);
      end = Number.parseInt(rangeParts[1], 10);
    } else {
      start = Number.parseInt(base, 10);
      end = start;
    }
  }
  if (!Number.isFinite(start) || !Number.isFinite(end)) return false;
  const clampedStart = Math.max(min, Math.min(max, start));
  const clampedEnd = Math.max(min, Math.min(max, end));
  if (value < clampedStart || value > clampedEnd) return false;
  return (value - clampedStart) % normalizedStep === 0;
}

function matchesCronField(field: string, value: number, min: number, max: number) {
  const atoms = splitCronField(field);
  if (!atoms.length) return true;
  return atoms.some((atom) => matchesCronAtom(atom, value, min, max));
}

function cronMatchesUtc(date: Date, expression: string) {
  const fields = String(expression || "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (fields.length !== 5) return false;
  const [minuteField, hourField, domField, monthField, dowField] = fields;
  const minute = date.getUTCMinutes();
  const hour = date.getUTCHours();
  const dom = date.getUTCDate();
  const month = date.getUTCMonth() + 1;
  const dow = date.getUTCDay();
  const minuteMatch = matchesCronField(minuteField, minute, 0, 59);
  const hourMatch = matchesCronField(hourField, hour, 0, 23);
  const monthMatch = matchesCronField(monthField, month, 1, 12);
  const domWildcard = !domField || domField === "*";
  const dowWildcard = !dowField || dowField === "*";
  const domMatch = domWildcard || matchesCronField(domField, dom, 1, 31);
  const dowMatch = dowWildcard || matchesCronField(dowField, dow === 0 ? 7 : dow, 0, 7);
  const dayMatch = domWildcard || dowWildcard ? domMatch && dowMatch : domMatch || dowMatch;
  return minuteMatch && hourMatch && monthMatch && dayMatch;
}

function expandCronOccurrences(expression: string, rangeStartMs: number, rangeEndMs: number) {
  const out: number[] = [];
  const start = Math.max(0, Math.min(rangeStartMs, rangeEndMs));
  const end = Math.max(rangeStartMs, rangeEndMs);
  const cursor = new Date(Math.floor(start / CALENDAR_SLOT_MS) * CALENDAR_SLOT_MS);
  while (cursor.getTime() < end) {
    if (cronMatchesUtc(cursor, expression)) out.push(cursor.getTime());
    cursor.setUTCMinutes(cursor.getUTCMinutes() + 1);
  }
  return out;
}

export function isCalendarEditableCron(expression: string) {
  const fields = String(expression || "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (fields.length !== 5) return false;
  const [minuteField, hourField, domField, monthField, dowField] = fields;
  const minuteOk = /^\d+$/.test(minuteField);
  const hourOk = /^\d+$/.test(hourField);
  const domOk = domField === "*";
  const monthOk = monthField === "*";
  const dowOk = dowField === "*" || /^[0-7]$/.test(dowField);
  return minuteOk && hourOk && domOk && monthOk && dowOk;
}

export function rewriteCronForDroppedStart(expression: string, start: Date) {
  const fields = String(expression || "")
    .trim()
    .split(/\s+/)
    .filter(Boolean);
  if (fields.length !== 5) return null;
  const [minuteField, hourField, domField, monthField, dowField] = fields;
  if (domField !== "*" || monthField !== "*") return null;
  if (!/^\d+$/.test(minuteField) || !/^\d+$/.test(hourField)) return null;
  const minute = String(start.getUTCMinutes()).padStart(2, "0");
  const hour = String(start.getUTCHours());
  const weekday = String(start.getUTCDay());
  const nextDowField = dowField === "*" ? "*" : weekday;
  return `${minute} ${hour} ${domField} ${monthField} ${nextDowField}`;
}

export function buildWorkflowCalendarOccurrences(
  automation: AutomationV2Spec,
  range: CalendarRange
): AutomationCalendarEvent[] {
  const automationId = String(automation?.automation_id || "").trim();
  const cronExpression = String(automation?.schedule?.cron_expression || "").trim();
  if (!automationId || automation?.schedule?.type !== "cron" || !cronExpression) return [];
  const starts = expandCronOccurrences(cronExpression, range.startMs, range.endMs);
  const editable = isCalendarEditableCron(cronExpression);
  const title = getAutomationCalendarTitle(automation);
  const status = String(automation.status || "active").trim() || "active";
  const scheduleLabel = formatAutomationV2ScheduleLabel(automation.schedule);
  return starts.map((startMs) => ({
    id: `${automationId}:${startMs}`,
    title,
    start: new Date(startMs),
    end: new Date(startMs + CALENDAR_DISPLAY_DURATION_MS),
    allDay: false,
    editable,
    startEditable: editable,
    durationEditable: false,
    extendedProps: {
      automation,
      automationId,
      scheduleLabel,
      cronExpression,
      status,
    },
  }));
}
