import { useEffect, useMemo, useState } from "react";
import { motion } from "framer-motion";
import { FolderOpen, Loader2, Sparkles } from "lucide-react";
import { open } from "@tauri-apps/plugin-dialog";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { cn } from "@/lib/utils";
import {
  installPack,
  installSkillTemplate,
  listPacks,
  listSkillTemplates,
  type PackMeta,
  type SkillLocation,
  type SkillTemplateInfo,
} from "@/lib/tauri";

interface PacksPanelProps {
  activeProjectPath?: string;
  onOpenInstalledPack?: (installedPath: string) => Promise<void> | void;
  onOpenSkills?: () => void;
}

export function PacksPanel({
  activeProjectPath,
  onOpenInstalledPack,
  onOpenSkills,
}: PacksPanelProps) {
  const [packs, setPacks] = useState<PackMeta[]>([]);
  const [loading, setLoading] = useState(true);
  const [installingId, setInstallingId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState("");

  // Starter skills (templates)
  const [templates, setTemplates] = useState<SkillTemplateInfo[]>([]);
  const [templatesLoading, setTemplatesLoading] = useState(false);
  const [installingTemplateId, setInstallingTemplateId] = useState<string | null>(null);
  const [showPackInfo, setShowPackInfo] = useState(false);
  const [skillLocation, setSkillLocation] = useState<SkillLocation>(
    activeProjectPath ? "project" : "global"
  );
  const projectName = activeProjectPath
    ? activeProjectPath.split(/[\\/]/).pop() || "Active Folder"
    : null;

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

  useEffect(() => {
    if (!activeProjectPath && skillLocation === "project") {
      setSkillLocation("global");
    }
  }, [activeProjectPath, skillLocation]);

  useEffect(() => {
    (async () => {
      try {
        setTemplatesLoading(true);
        setTemplates(await listSkillTemplates());
      } catch (e) {
        console.warn("Failed to load starter skills:", e);
      } finally {
        setTemplatesLoading(false);
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

  const handleInstallTemplate = async (templateId: string) => {
    try {
      setInstallingTemplateId(templateId);
      setError(null);
      await installSkillTemplate(templateId, skillLocation);
      // Installing a skill is "silent success"; users can see it under Extensions -> Skills.
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg || "Failed to install starter skill");
    } finally {
      setInstallingTemplateId(null);
    }
  };

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

        <div className="mt-10 space-y-4">
          <div className="flex flex-col gap-2 sm:flex-row sm:items-end sm:justify-between">
            <div>
              <h2 className="text-lg font-semibold text-text terminal-text">Starter skills</h2>
              <p className="text-sm text-text-muted">
                Add a few offline “capabilities” that help Tandem help you.
              </p>
            </div>
            <div className="flex flex-wrap items-center gap-2">
              {activeProjectPath ? (
                <div className="flex items-center gap-2 rounded-lg border border-border bg-surface-elevated px-3 py-2 text-xs text-text-muted">
                  <span>Install to:</span>
                  <button
                    type="button"
                    onClick={() => setSkillLocation("project")}
                    className={cn(
                      "rounded-md px-2 py-1 transition-colors",
                      skillLocation === "project"
                        ? "bg-primary/20 text-primary"
                        : "hover:bg-surface"
                    )}
                  >
                    Folder ({projectName})
                  </button>
                  <button
                    type="button"
                    onClick={() => setSkillLocation("global")}
                    className={cn(
                      "rounded-md px-2 py-1 transition-colors",
                      skillLocation === "global" ? "bg-primary/20 text-primary" : "hover:bg-surface"
                    )}
                  >
                    Global
                  </button>
                </div>
              ) : (
                <div className="text-xs text-text-subtle">Install to: Global</div>
              )}

              {onOpenSkills && (
                <Button variant="secondary" size="sm" onClick={onOpenSkills} className="h-8">
                  Manage Skills
                </Button>
              )}
            </div>
          </div>

          {templatesLoading ? (
            <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
              Loading starter skills...
            </div>
          ) : templates.length === 0 ? (
            <div className="rounded-lg border border-border bg-surface-elevated p-4 text-sm text-text-muted">
              No starter skills found.
            </div>
          ) : (
            <div className="grid gap-4 md:grid-cols-2">
              {templates.map((t) => (
                <div key={t.id} className="glass border-glass ring-1 ring-white/5 p-5">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <p className="truncate text-sm font-semibold text-text">{t.name}</p>
                      <p className="mt-1 text-xs text-text-muted">{t.description}</p>
                    </div>
                    <Button
                      size="sm"
                      onClick={() => handleInstallTemplate(t.id)}
                      disabled={!!installingTemplateId}
                    >
                      {installingTemplateId === t.id ? "Installing..." : "Install"}
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}

          <p className="text-xs text-text-subtle">
            Tip: "Advanced: paste SKILL.md" is still available in Extensions {"->"} Skills.
          </p>
        </div>
      </div>
    </div>
  );
}
