import { useCallback, useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { AlertCircle, CheckCircle2, ExternalLink, FolderOpen, Loader2, Wrench } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/utils";
import {
  pythonCreateVenv,
  pythonGetStatus,
  pythonInstallRequirements,
  readDirectory,
  type PythonInstallResult,
  type PythonStatus,
} from "@/lib/tauri";

type OsTab = "windows" | "macos" | "linux";

function detectOs(): OsTab {
  const p = (globalThis.navigator?.platform || "").toLowerCase();
  const ua = (globalThis.navigator?.userAgent || "").toLowerCase();
  if (p.includes("win") || ua.includes("windows")) return "windows";
  if (p.includes("mac") || ua.includes("mac os")) return "macos";
  return "linux";
}

function CodeBlock({ children }: { children: string }) {
  return (
    <pre className="mt-2 overflow-auto rounded-lg border border-border bg-surface px-3 py-2 text-[11px] text-text font-mono">
      {children}
    </pre>
  );
}

export function PythonSetupWizard({ onClose }: { onClose: () => void }) {
  const [status, setStatus] = useState<PythonStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [osTab, setOsTab] = useState<OsTab>(() => detectOs());
  const [installLog, setInstallLog] = useState<PythonInstallResult | null>(null);

  const refresh = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      setStatus(await pythonGetStatus());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const bestCandidateLabel = useMemo(() => {
    if (!status?.candidates?.length) return "not found";
    return `${status.candidates[0].kind} (${status.candidates[0].version})`;
  }, [status?.candidates]);

  const canCreateVenv = !!status?.found && !!status?.workspace_path;
  const canInstallReqs = !!status?.venv_exists && !!status?.workspace_path;

  const handleCreateVenv = useCallback(async () => {
    try {
      setBusy("create");
      setError(null);
      setInstallLog(null);
      const next = await pythonCreateVenv();
      setStatus(next);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }, []);

  const handleOpenVenv = useCallback(async () => {
    const venvRoot = status?.venv_root;
    if (!venvRoot) return;
    try {
      await openPath(venvRoot);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  }, [status?.venv_root]);

  const handleInstallRequirements = useCallback(async () => {
    try {
      setBusy("install");
      setError(null);
      setInstallLog(null);

      const ws = status?.workspace_path;
      if (!ws) {
        setError("No workspace selected.");
        return;
      }
      const sep = ws.includes("\\") ? "\\" : "/";

      // Try to auto-detect a requirements file in the workspace root.
      // This keeps the UX simple for the common case (`requirements.txt` in the project).
      let selection: string | null = null;
      try {
        const entries = await readDirectory(ws);
        const reqCandidates = entries
          .filter((e) => !e.is_directory)
          .filter((e) => /^requirements.*\.(txt|in)$/i.test(e.name))
          .sort((a, b) => {
            // Prefer the canonical `requirements.txt` if it exists.
            const aIsCanonical = a.name.toLowerCase() === "requirements.txt";
            const bIsCanonical = b.name.toLowerCase() === "requirements.txt";
            if (aIsCanonical !== bIsCanonical) return aIsCanonical ? -1 : 1;
            return a.name.localeCompare(b.name);
          });
        selection = reqCandidates[0]?.path ?? null;
      } catch {
        // If directory listing fails for any reason, fall back to the file picker.
        selection = null;
      }

      if (!selection) {
        const picked = await open({
          title: "Select a requirements file (must be inside the workspace)",
          multiple: false,
          directory: false,
          defaultPath: `${ws}${ws.endsWith("\\") || ws.endsWith("/") ? "" : sep}requirements.txt`,
          filters: [{ name: "Python requirements", extensions: ["txt", "in"] }],
        });
        if (!picked || typeof picked !== "string") return;
        selection = picked;
      }
      if (!selection || typeof selection !== "string") return;

      const result = await pythonInstallRequirements(selection);
      setInstallLog(result);
      await refresh();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }, [refresh, status?.workspace_path]);

  const installInstructions = useMemo(() => {
    const ws = status?.workspace_path ?? "<your-workspace>";
    return {
      windows: {
        title: "Windows",
        steps: [
          "Install Python 3 from python.org, or via your preferred package manager.",
          "Re-open Tandem and click Create venv.",
        ],
        commands: `# Optional: verify Python is installed\npython --version\npy -3 --version\n\n# Create a workspace venv\ncd "${ws}"\npython -m venv .opencode\\.venv\n\n# Install requirements inside the venv\n.opencode\\.venv\\Scripts\\python.exe -m pip install -r requirements.txt`,
        links: [
          { label: "python.org downloads", url: "https://www.python.org/downloads/" },
          {
            label: "winget python",
            url: "https://learn.microsoft.com/windows/package-manager/winget/",
          },
        ],
      },
      macos: {
        title: "macOS",
        steps: [
          "Install Python 3 (recommended: Homebrew) or use the official installer.",
          "Re-open Tandem and click Create venv.",
        ],
        commands: `# Optional: verify Python is installed\npython3 --version\n\n# Create a workspace venv\ncd "${ws}"\npython3 -m venv .opencode/.venv\n\n# Install requirements inside the venv\n.opencode/.venv/bin/python3 -m pip install -r requirements.txt`,
        links: [
          { label: "Homebrew", url: "https://brew.sh/" },
          { label: "python.org macOS", url: "https://www.python.org/downloads/macos/" },
        ],
      },
      linux: {
        title: "Linux",
        steps: [
          "Install Python 3 via your distro package manager.",
          "Re-open Tandem and click Create venv.",
        ],
        commands: `# Optional: verify Python is installed\npython3 --version\n\n# Create a workspace venv\ncd "${ws}"\npython3 -m venv .opencode/.venv\n\n# Install requirements inside the venv\n.opencode/.venv/bin/python3 -m pip install -r requirements.txt`,
        links: [{ label: "Python docs: venv", url: "https://docs.python.org/3/library/venv.html" }],
      },
    } as const;
  }, [status?.workspace_path]);

  const currentInst = installInstructions[osTab];

  return (
    <AnimatePresence>
      <motion.div
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
        className="fixed inset-0 z-50"
      >
        {/* Backdrop */}
        <div className="absolute inset-0 bg-black/60 backdrop-blur-sm" onClick={onClose} />

        {/* Modal */}
        <div className="absolute inset-0 flex items-center justify-center p-4">
          <motion.div
            initial={{ opacity: 0, scale: 0.98, y: 10 }}
            animate={{ opacity: 1, scale: 1, y: 0 }}
            exit={{ opacity: 0, scale: 0.98, y: 10 }}
            transition={{ duration: 0.15 }}
            className={cn(
              "bg-surface border-glass rounded-xl shadow-2xl w-full max-w-3xl overflow-hidden",
              "ring-1 ring-white/5"
            )}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="flex items-center justify-between gap-3 border-b border-border px-5 py-4">
              <div className="flex items-center gap-3 min-w-0">
                <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-primary/15 text-primary">
                  <Wrench className="h-5 w-5" />
                </div>
                <div className="min-w-0">
                  <h2 className="truncate text-base font-semibold text-text">
                    Python Setup (Workspace Venv)
                  </h2>
                  <p className="truncate text-xs text-text-muted">
                    Enforces `.opencode/.venv` so the AI never installs global packages.
                  </p>
                </div>
              </div>
              <Button variant="ghost" size="sm" onClick={onClose} className="h-8">
                Close
              </Button>
            </div>

            <div className="p-5">
              {loading ? (
                <div className="flex items-center gap-2 text-sm text-text-muted">
                  <Loader2 className="h-4 w-4 animate-spin" />
                  Checking Python...
                </div>
              ) : (
                <>
                  {/* Status */}
                  <div className="grid gap-3 md:grid-cols-3">
                    <div className="rounded-lg border border-border bg-surface-elevated/50 p-3">
                      <p className="text-xs text-text-subtle">Python</p>
                      <div className="mt-1 flex items-center gap-2">
                        {status?.found ? (
                          <CheckCircle2 className="h-4 w-4 text-success" />
                        ) : (
                          <AlertCircle className="h-4 w-4 text-warning" />
                        )}
                        <p className="text-sm font-medium text-text">
                          {status?.found ? "Detected" : "Not detected"}
                        </p>
                      </div>
                      <p className="mt-1 text-xs text-text-muted">
                        Best candidate: {bestCandidateLabel}
                      </p>
                    </div>

                    <div className="rounded-lg border border-border bg-surface-elevated/50 p-3">
                      <p className="text-xs text-text-subtle">Workspace</p>
                      <p className="mt-1 text-xs text-text-muted break-all">
                        {status?.workspace_path ?? "No workspace selected"}
                      </p>
                    </div>

                    <div className="rounded-lg border border-border bg-surface-elevated/50 p-3">
                      <p className="text-xs text-text-subtle">Venv</p>
                      <div className="mt-1 flex items-center gap-2">
                        {status?.venv_exists ? (
                          <CheckCircle2 className="h-4 w-4 text-success" />
                        ) : (
                          <AlertCircle className="h-4 w-4 text-warning" />
                        )}
                        <p className="text-sm font-medium text-text">
                          {status?.venv_exists ? "Ready" : "Missing"}
                        </p>
                      </div>
                      <p className="mt-1 text-xs text-text-muted break-all">
                        {status?.venv_root ?? ".opencode/.venv"}
                      </p>
                    </div>
                  </div>

                  {/* Actions */}
                  <div className="mt-4 flex flex-wrap items-center gap-2">
                    <Button onClick={refresh} variant="secondary" size="sm" className="h-8">
                      Refresh
                    </Button>

                    <Button
                      onClick={handleCreateVenv}
                      disabled={!canCreateVenv || busy !== null}
                      size="sm"
                      className="h-8"
                    >
                      {busy === "create" ? (
                        <>
                          <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                          Creating...
                        </>
                      ) : (
                        "Create venv in workspace"
                      )}
                    </Button>

                    <Button
                      onClick={handleInstallRequirements}
                      disabled={!canInstallReqs || busy !== null}
                      variant="secondary"
                      size="sm"
                      className="h-8"
                    >
                      {busy === "install" ? (
                        <>
                          <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                          Installing...
                        </>
                      ) : (
                        "Install requirements..."
                      )}
                    </Button>

                    <Button
                      onClick={handleOpenVenv}
                      disabled={!status?.venv_root}
                      variant="ghost"
                      size="sm"
                      className="h-8"
                    >
                      <FolderOpen className="mr-2 h-4 w-4" />
                      Open venv folder
                    </Button>
                  </div>

                  {/* Errors */}
                  {error && (
                    <div className="mt-4 rounded-lg border border-error/20 bg-error/10 p-3 text-sm text-error">
                      {error}
                    </div>
                  )}

                  {/* Missing python instructions */}
                  {!status?.found && (
                    <div className="mt-4 rounded-lg border border-border bg-surface-elevated/50 p-4">
                      <div className="flex items-center justify-between gap-2">
                        <p className="text-sm font-medium text-text">Install Python</p>
                        <div className="flex items-center gap-1">
                          {(["windows", "macos", "linux"] as const).map((t) => (
                            <button
                              key={t}
                              onClick={() => setOsTab(t)}
                              className={cn(
                                "rounded-md px-2 py-1 text-xs transition-colors",
                                osTab === t
                                  ? "bg-primary/15 text-primary"
                                  : "text-text-muted hover:text-text hover:bg-surface"
                              )}
                            >
                              {installInstructions[t].title}
                            </button>
                          ))}
                        </div>
                      </div>

                      <ul className="mt-3 list-disc pl-4 text-xs text-text-muted space-y-1">
                        {currentInst.steps.map((s) => (
                          <li key={s}>{s}</li>
                        ))}
                      </ul>

                      <div className="mt-3 flex flex-wrap gap-2">
                        {currentInst.links.map((l) => (
                          <a
                            key={l.url}
                            href={l.url}
                            target="_blank"
                            rel="noreferrer"
                            className="inline-flex items-center gap-1 rounded-md border border-border bg-surface px-2 py-1 text-xs text-text-muted hover:text-text"
                          >
                            <ExternalLink className="h-3 w-3" />
                            {l.label}
                          </a>
                        ))}
                      </div>

                      <CodeBlock>{currentInst.commands}</CodeBlock>
                    </div>
                  )}

                  {/* Install logs */}
                  {installLog && (
                    <div className="mt-4 rounded-lg border border-border bg-surface-elevated/50 p-4">
                      <p className="text-sm font-medium text-text">
                        Install result:{" "}
                        <span className={installLog.ok ? "text-success" : "text-error"}>
                          {installLog.ok ? "OK" : "Failed"}
                        </span>
                      </p>
                      <p className="mt-1 text-xs text-text-muted">
                        Exit code: {installLog.exit_code ?? "unknown"}
                      </p>
                      <div className="mt-3 grid gap-3 md:grid-cols-2">
                        <div>
                          <p className="text-xs font-medium text-text-muted">stdout</p>
                          <CodeBlock>{installLog.stdout || "(empty)"}</CodeBlock>
                        </div>
                        <div>
                          <p className="text-xs font-medium text-text-muted">stderr</p>
                          <CodeBlock>{installLog.stderr || "(empty)"}</CodeBlock>
                        </div>
                      </div>
                    </div>
                  )}
                </>
              )}
            </div>
          </motion.div>
        </div>
      </motion.div>
    </AnimatePresence>
  );
}
