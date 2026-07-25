function normalizeStatus(value) {
  return String(value || "").trim().toUpperCase();
}

export function classifyStatusOnlyWorkspaceChange(beforeStatus, afterStatus) {
  const before = normalizeStatus(beforeStatus);
  const after = normalizeStatus(afterStatus);
  if (before === after) return null;
  if (after === "D") return "deleted";
  if (before || after) return "updated";
  return null;
}
