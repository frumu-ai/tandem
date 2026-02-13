import type { Session } from "@/lib/tauri";

function normalizePath(path: string): string {
  return path.trim().replace(/\\/g, "/").replace(/\/+/g, "/").replace(/\/$/, "").toLowerCase();
}

function isDotPath(path: string | null | undefined): boolean {
  if (!path) return true;
  const trimmed = path.trim();
  return trimmed === "." || trimmed === "./" || trimmed === ".\\";
}

export function resolveSessionDirectory(
  sessionDirectory: string | null | undefined,
  workspacePath: string | null | undefined
): string {
  if (isDotPath(sessionDirectory)) {
    return workspacePath?.trim() || ".";
  }
  return sessionDirectory?.trim() || ".";
}

export function sessionBelongsToWorkspace(
  session: Pick<Session, "directory">,
  workspacePath: string | null | undefined
): boolean {
  if (!workspacePath?.trim()) return true;

  const resolvedDirectory = resolveSessionDirectory(session.directory, workspacePath);
  if (resolvedDirectory === ".") {
    return false;
  }

  const normalizedSession = normalizePath(resolvedDirectory);
  const normalizedWorkspace = normalizePath(workspacePath);

  return (
    normalizedSession === normalizedWorkspace ||
    normalizedSession.startsWith(`${normalizedWorkspace}/`) ||
    normalizedWorkspace.startsWith(`${normalizedSession}/`)
  );
}
