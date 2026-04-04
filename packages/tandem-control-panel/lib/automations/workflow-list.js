const WORKFLOW_SORT_MODES = [
  { value: "created_desc", label: "Created: newest first" },
  { value: "created_asc", label: "Created: oldest first" },
  { value: "name_asc", label: "Name: A to Z" },
  { value: "name_desc", label: "Name: Z to A" },
];

const DEFAULT_WORKFLOW_SORT_MODE = WORKFLOW_SORT_MODES[0].value;

function normalizeWorkflowSortMode(raw) {
  const value = String(raw || "").trim().toLowerCase();
  if (WORKFLOW_SORT_MODES.some((mode) => mode.value === value)) return value;
  return DEFAULT_WORKFLOW_SORT_MODE;
}

function getAutomationId(row) {
  return String(row?.automation_id || row?.automationId || row?.id || row?.routine_id || "").trim();
}

function getAutomationName(row) {
  return String(row?.name || row?.title || row?.label || getAutomationId(row) || "Automation").trim();
}

function toNumber(value) {
  const n = Number(value);
  return Number.isFinite(n) ? n : 0;
}

function parseTimestampValue(value) {
  if (!value) return 0;
  if (typeof value === "number") return Number.isFinite(value) ? value : 0;
  if (typeof value === "string") {
    const raw = value.trim();
    if (!raw) return 0;
    const numeric = Number(raw);
    if (Number.isFinite(numeric) && raw === String(numeric)) return numeric;
    const parsed = Date.parse(raw);
    return Number.isFinite(parsed) ? parsed : 0;
  }
  return 0;
}

function getAutomationCreatedAtMs(row) {
  const candidates = [
    row?.created_at_ms,
    row?.createdAtMs,
    row?.created_at,
    row?.createdAt,
    row?.updated_at_ms,
    row?.updatedAtMs,
    row?.updated_at,
    row?.updatedAt,
  ];
  for (const candidate of candidates) {
    const parsed = parseTimestampValue(candidate);
    if (parsed > 0) return parsed;
  }
  return 0;
}

function formatAutomationCreatedAtLabel(row) {
  const createdAtMs = getAutomationCreatedAtMs(row);
  if (!createdAtMs) return "";
  const date = new Date(createdAtMs);
  const datePart = new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  }).format(date);
  const timePart = new Intl.DateTimeFormat(undefined, {
    hour: "numeric",
    minute: "2-digit",
  }).format(date);
  return `${datePart} · ${timePart}`;
}

function normalizeFavoriteAutomationIds(ids) {
  const seen = new Set();
  const out = [];
  for (const raw of Array.isArray(ids) ? ids : []) {
    const id = String(raw || "").trim();
    if (!id || seen.has(id)) continue;
    seen.add(id);
    out.push(id);
  }
  return out;
}

function toggleFavoriteAutomationId(ids, automationId) {
  const id = String(automationId || "").trim();
  if (!id) return normalizeFavoriteAutomationIds(ids);
  const current = normalizeFavoriteAutomationIds(ids);
  if (current.includes(id)) {
    return current.filter((rowId) => rowId !== id);
  }
  return [...current, id];
}

function compareAutomationRows(left, right, sortMode, favoriteSet) {
  const leftId = getAutomationId(left);
  const rightId = getAutomationId(right);
  const leftFavorite = !!favoriteSet?.has?.(leftId);
  const rightFavorite = !!favoriteSet?.has?.(rightId);
  if (leftFavorite !== rightFavorite) return leftFavorite ? -1 : 1;

  const leftName = getAutomationName(left);
  const rightName = getAutomationName(right);
  const leftCreated = getAutomationCreatedAtMs(left);
  const rightCreated = getAutomationCreatedAtMs(right);

  if (sortMode === "name_asc" || sortMode === "name_desc") {
    const cmp = leftName.localeCompare(rightName, undefined, { sensitivity: "base" });
    if (cmp) return sortMode === "name_desc" ? -cmp : cmp;
    if (leftCreated !== rightCreated) return rightCreated - leftCreated;
    return leftId.localeCompare(rightId);
  }

  if (leftCreated !== rightCreated) {
    return sortMode === "created_asc" ? leftCreated - rightCreated : rightCreated - leftCreated;
  }
  const cmp = leftName.localeCompare(rightName, undefined, { sensitivity: "base" });
  if (cmp) return cmp;
  return leftId.localeCompare(rightId);
}

function sortWorkflowAutomations(rows, options = {}) {
  const sortMode = normalizeWorkflowSortMode(options.sortMode);
  const favoriteSet =
    options.favoriteAutomationIds instanceof Set
      ? options.favoriteAutomationIds
      : new Set(normalizeFavoriteAutomationIds(options.favoriteAutomationIds));
  return Array.isArray(rows) ? [...rows].sort((left, right) => compareAutomationRows(left, right, sortMode, favoriteSet)) : [];
}

export {
  DEFAULT_WORKFLOW_SORT_MODE,
  WORKFLOW_SORT_MODES,
  compareAutomationRows,
  getAutomationCreatedAtMs,
  getAutomationId,
  getAutomationName,
  formatAutomationCreatedAtLabel,
  normalizeFavoriteAutomationIds,
  normalizeWorkflowSortMode,
  sortWorkflowAutomations,
  toggleFavoriteAutomationId,
};
