import { useEffect, useMemo, useState } from "react";
import { motion } from "framer-motion";
import { FolderOpen, Loader2, Sparkles } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { cn } from "@/lib/utils";
import { installPack, listPacks, type PackMeta } from "@/lib/tauri";
import { PythonSetupWizard } from "@/components/python";

interface PacksPanelProps {
  activeProjectPath?: string;
  onOpenInstalledPack?: (installedPath: string) => Promise<void> | void;
  onOpenSkills?: () => void;
}

export function PacksPanel({
  activeProjectPath: _activeProjectPath,
  onOpenInstalledPack,
  onOpenSkills,
}: PacksPanelProps) {
  const [packs, setPacks] = useState<PackMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  const [showPackInfo, setShowPackInfo] = useState(false);
  const [showPythonWizard, setShowPythonWizard] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        setLoading(true);
        setError(null);
        setPacks(await listPacks());
      } catch (e) {
        setError(e instanceof Error ? e.message : "Failed to load packs");
      } finally {
        setLoading(false);
      }
    })();
  }, []);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return packs;
    return packs.filter((p) => {
      const haystack = [p.title, p.description, p.complexity, p.time_estimate, ...(p.tags ?? [])]
        .join(" ")
        .toLowerCase();
      return haystack.includes(q);
    });
  }, [packs, query]);

  const handleInstall = async (packId: string) => {
    try {
      setInstallingId(packId);
      setError(null);

      const destination = await open({
        directory: true,
        multiple: false,
        title: "Choose where to create the starter pack folder",
      });

      if (!destination || typeof destination !== "string") return;

      const result = await installPack(packId, destination);
      await onOpenInstalledPack?.(result.installed_path);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg || "Failed to install pack");
    } finally {
      setInstallingId(null);
    }
  };

  return (
    <div className="flex h-full w-full flex-col overflow-y-auto">
      <div className="mx-auto w-full max-w-5xl p-8">
        <motion.div
          className="mb-8 flex flex-col gap-2"
          initial={{ opacity: 0, y: 10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.25 }}
        >
          <div className="flex items-center gap-3">
            <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-primary/20 text-primary">
              <Sparkles className="h-5 w-5" />
            </div>
            <div>
              <h1 className="text-2xl font-bold text-text terminal-text">Starter Packs</h1>
              <p className="text-sm text-text-muted">
                Guided, copyable folders for real-world tasks.
              </p>
            </div>
          </div>

          <div className="mt-2 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="max-w-md flex-1">
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder="Search packs (research, writing, security...)"
              />
            </div>
            <div className="flex items-center gap-3 text-xs text-text-subtle">
              <span>
                <FolderOpen className="mr-1 inline h-3 w-3" />
                Installs include START_HERE.md, prompts, and sample inputs.
              </span>
              {onOpenSkills && (
                <Button variant="ghost" size="sm" onClick={onOpenSkills} className="h-8 px-2">
                  Open Skills
                </Button>
              )}
            </div>
          </div>
          <div className="mt-2">
            <button
              type="button"
              onClick={() => setShowPackInfo((v) => !v)}
              className="text-xs text-text-subtle hover:text-text underline underline-offset-4"
            >
              {showPackInfo ? "Hide pack details" : "What are packs and where do they install?"}
            </button>
            {showPackInfo && (
              <div className="mt-2 rounded-lg border border-border bg-surface-elevated p-3 text-xs text-text-muted">
                <p>
                  Packs are guided, copyable folders with prompts and expected outputs. After
                  install, open START_HERE.md to follow the workflow.
                </p>
                <p className="mt-2">
                  Click Install to choose a clean location and create the pack folder there.
                </p>
              </div>
            )}
          </div>
        </motion.div>

        <div className="mb-6 rounded-lg border border-border bg-surface-elevated/50 p-3 text-xs text-text-muted">
          <div className="flex items-center justify-between gap-3">
            <p className="font-medium text-text">Runtime note</p>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => setShowPythonWizard(true)}
              className="h-7 px-2 text-[11px]"
            >
              Set up Python (venv)
            </Button>
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <span className="text-[11px] text-text-subtle">Some packs may require:</span>
            <span className="rounded-full border border-yellow-500/20 bg-yellow-500/10 px-2 py-0.5 text-xs text-yellow-500">
              Python
            </span>
            <span className="rounded-full border border-emerald-500/20 bg-emerald-500/10 px-2 py-0.5 text-xs text-emerald-200">
              Node
            </span>
            <span className="rounded-full border border-sky-500/20 bg-sky-500/10 px-2 py-0.5 text-xs text-sky-200">
              Bash
            </span>
          </div>
          <p className="mt-1">
            Packs can include scripts and local tooling. Tandem does not bundle these runtimes. If a
            pack asks you to run a tool you don’t have, install it locally or choose a different
            pack.
          </p>
        </div>

        {error && (
          <div className="mb-6 rounded-lg border border-error/20 bg-error/10 p-3 text-sm text-error">
            {error}
          </div>
        )}

        {loading ? (
          <div className="flex items-center justify-center py-16 text-text-muted">
            <Loader2 className="mr-2 h-5 w-5 animate-spin" />
            Loading packs...
          </div>
        ) : filtered.length === 0 ? (
          <div className="rounded-lg border border-border bg-surface-elevated p-8 text-center">
            <p className="text-sm text-text-muted">No packs match your search.</p>
          </div>
        ) : (
          <div className="grid gap-4 md:grid-cols-2">
            {filtered.map((pack) => {
              const isInstalling = installingId === pack.id;
              return (
                <div
                  key={pack.id}
                  className="glass border-glass overflow-hidden ring-1 ring-white/5"
                >
                  <div className="p-5">
                    <div className="flex items-start justify-between gap-4">
                      <div className="min-w-0">
                        <h3 className="truncate text-base font-semibold text-text">{pack.title}</h3>
                        <p className="mt-1 text-sm text-text-muted">{pack.description}</p>
                      </div>
                      <div className="flex flex-shrink-0 items-center gap-2">
                        <Button onClick={() => handleInstall(pack.id)} disabled={isInstalling}>
                          {isInstalling ? (
                            <>
                              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                              Installing...
                            </>
                          ) : (
                            "Install"
                          )}
                        </Button>
                      </div>
                    </div>

                    <div className="mt-4 flex flex-wrap items-center gap-2 text-xs">
                      <span className="rounded-full bg-primary/15 px-2 py-1 text-primary">
                        {pack.complexity}
                      </span>
                      <span className="rounded-full bg-surface-elevated px-2 py-1 text-text-subtle">
                        {pack.time_estimate}
                      </span>
                      {(pack.tags ?? []).slice(0, 4).map((t) => (
                        <span
                          key={t}
                          className={cn(
                            "rounded-full px-2 py-1",
                            "bg-surface-elevated text-text-subtle"
                          )}
                        >
                          {t}
                        </span>
                      ))}
                    </div>
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>
      {showPythonWizard && <PythonSetupWizard onClose={() => setShowPythonWizard(false)} />}
    </div>
  );
}
