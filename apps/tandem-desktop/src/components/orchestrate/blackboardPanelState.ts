export type BlackboardPanelMode = "docked" | "expanded" | "fullscreen";

export function toggleExpand(mode: BlackboardPanelMode): BlackboardPanelMode {
  return mode === "docked" ? "expanded" : "docked";
}

export function toggleFullscreen(mode: BlackboardPanelMode): BlackboardPanelMode {
  return mode === "fullscreen" ? "expanded" : "fullscreen";
}

export function applyEsc(mode: BlackboardPanelMode): BlackboardPanelMode {
  return mode === "fullscreen" ? "expanded" : mode;
}

export function reconcileSelection<T extends { id: string }>(
  selectedId: string | null,
  rows: T[]
): string | null {
  if (rows.length === 0) return null;
  if (!selectedId) return rows[0].id;
  if (rows.some((row) => row.id === selectedId)) return selectedId;
  return rows[0].id;
}

export function pauseFollowOnManualNavigation(isFollowEnabled: boolean): boolean {
  return isFollowEnabled ? false : false;
}

export function shouldAutoFocusOnDecision(
  followEnabled: boolean,
  newestDecisionSeq: number | null,
  lastAutoFocusedDecisionSeq: number
): boolean {
  if (!followEnabled) return false;
  if (newestDecisionSeq === null) return false;
  return newestDecisionSeq > lastAutoFocusedDecisionSeq;
}
