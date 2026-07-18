export const CRON_WEEKDAY_ALIASES: Readonly<Record<string, number>> = {
  sun: 7,
  mon: 1,
  tue: 2,
  wed: 3,
  thu: 4,
  fri: 5,
  sat: 6,
};

function parseCronValue(raw: string, aliases?: Readonly<Record<string, number>>) {
  const normalized = String(raw || "")
    .trim()
    .toLowerCase();
  if (aliases && normalized in aliases) return aliases[normalized];
  if (!/^\d+$/.test(normalized)) return Number.NaN;
  return Number.parseInt(normalized, 10);
}

function matchesCronAtom(
  atom: string,
  value: number,
  min: number,
  max: number,
  aliases?: Readonly<Record<string, number>>
) {
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
      start = parseCronValue(rangeParts[0], aliases);
      end = parseCronValue(rangeParts[1], aliases);
    } else {
      start = parseCronValue(base, aliases);
      end = start;
    }
  }
  if (!Number.isFinite(start) || !Number.isFinite(end)) return false;
  const clampedStart = Math.max(min, Math.min(max, start));
  const clampedEnd = Math.max(min, Math.min(max, end));
  if (value < clampedStart || value > clampedEnd) return false;
  return (value - clampedStart) % normalizedStep === 0;
}

export function matchesCronField(
  field: string,
  value: number,
  min: number,
  max: number,
  aliases?: Readonly<Record<string, number>>
) {
  const atoms = String(field || "")
    .trim()
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
  if (!atoms.length) return true;
  return atoms.some((atom) => matchesCronAtom(atom, value, min, max, aliases));
}
