function normalizeStatus(value) {
  return String(value || "")
    .trim()
    .toUpperCase();
}

export function classifyStatusOnlyWorkspaceChange(
  beforeStatus,
  afterStatus,
  beforeFingerprint = "",
  afterFingerprint = ""
) {
  const before = normalizeStatus(beforeStatus);
  const after = normalizeStatus(afterStatus);
  if (before === after && String(beforeFingerprint || "") === String(afterFingerprint || "")) {
    return null;
  }
  if (after === "D") return "deleted";
  if (before || after || beforeFingerprint || afterFingerprint) return "updated";
  return null;
}
