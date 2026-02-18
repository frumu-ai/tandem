import { useEffect, useMemo, useState } from "react";
import { motion } from "framer-motion";
import { FolderOpen, Loader2, Sparkles } from "lucide-react";
import { useTranslation } from "react-i18next";
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
  const { t } = useTranslation("common");
  const localizePackField = (
    pack: PackMeta,
    field: "title" | "description" | "complexity" | "time_estimate"
  ): string => {
    const key = `packs.catalog.${pack.id}.${field}`;
    const value = t(key);
    // i18next returns the key itself when not found.
    return value === key ? pack[field] : value;
  };

  const localizePackTag = (tag: string): string => {
    const key = `packs.tags.${tag}`;
    const value = t(key);
    return value === key ? tag : value;
  };
  const runtimePillClass = (runtime: string) => {
    switch (runtime.toLowerCase()) {
      case "python":
        return "border-yellow-500/20 bg-yellow-500/10 text-yellow-500";
      case "node":
        return "border-emerald-500/20 bg-emerald-500/10 text-emerald-200";
      case "bash":
        return "border-sky-500/20 bg-sky-500/10 text-sky-200";
      default:
        return "border-border bg-surface-elevated text-text-subtle";
    }
  };

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
        setError(e instanceof Error ? e.message : t("packs.errors.loadFailed"));
      } finally {
        setLoading(false);
      }
    })();
  }, [t]);

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
        title: t("packs.dialog.chooseInstallFolder"),
      });

      if (!destination || typeof destination !== "string") return;

      const result = await installPack(packId, destination);
      await onOpenInstalledPack?.(result.installed_path);
    } catch (e) {
      const msg = e instanceof Error ? e.message : String(e);
      setError(msg || t("packs.errors.installFailed"));
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
              <h1 className="text-2xl font-bold text-text terminal-text">{t("packs.title")}</h1>
              <p className="text-sm text-text-muted">{t("packs.subtitle")}</p>
            </div>
          </div>

          <div className="mt-2 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
            <div className="max-w-md flex-1">
              <Input
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("packs.searchPlaceholder")}
              />
            </div>
            <div className="flex items-center gap-3 text-xs text-text-subtle">
              <span>
                <FolderOpen className="mr-1 inline h-3 w-3" />
                {t("packs.installsInclude")}
              </span>
              {onOpenSkills && (
                <Button variant="ghost" size="sm" onClick={onOpenSkills} className="h-8 px-2">
                  {t("packs.openSkills")}
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
              {showPackInfo ? t("packs.hideDetails") : t("packs.showDetails")}
            </button>
            {showPackInfo && (
              <div className="mt-2 rounded-lg border border-border bg-surface-elevated p-3 text-xs text-text-muted">
                <p>{t("packs.detailsBody")}</p>
                <p className="mt-2">{t("packs.installHint")}</p>
              </div>
            )}
          </div>
        </motion.div>

        <div className="mb-6 rounded-lg border border-border bg-surface-elevated/50 p-3 text-xs text-text-muted">
          <div className="flex items-center justify-between gap-3">
            <p className="font-medium text-text">{t("packs.runtimeNoteTitle")}</p>
            <Button
              variant="secondary"
              size="sm"
              onClick={() => setShowPythonWizard(true)}
              className="h-7 px-2 text-[11px]"
            >
              {t("packs.setupPython")}
            </Button>
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-2">
            <span className="text-[11px] text-text-subtle">{t("packs.requiresLabel")}</span>
            <span
              className={cn("rounded-full border px-2 py-0.5 text-xs", runtimePillClass("python"))}
            >
              Python
            </span>
            <span
              className={cn("rounded-full border px-2 py-0.5 text-xs", runtimePillClass("node"))}
            >
              Node
            </span>
            <span
              className={cn("rounded-full border px-2 py-0.5 text-xs", runtimePillClass("bash"))}
            >
              Bash
            </span>
          </div>
          <p className="mt-1">{t("packs.runtimeNoteBody")}</p>
        </div>

        {error && (
          <div className="mb-6 rounded-lg border border-error/20 bg-error/10 p-3 text-sm text-error">
            {error}
          </div>
        )}

        {loading ? (
          <div className="flex items-center justify-center py-16 text-text-muted">
            <Loader2 className="mr-2 h-5 w-5 animate-spin" />
            {t("packs.loading")}
          </div>
        ) : filtered.length === 0 ? (
          <div className="rounded-lg border border-border bg-surface-elevated p-8 text-center">
            <p className="text-sm text-text-muted">{t("packs.emptySearch")}</p>
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
                        <h3 className="truncate text-base font-semibold text-text">
                          {localizePackField(pack, "title")}
                        </h3>
                        <p className="mt-1 text-sm text-text-muted">
                          {localizePackField(pack, "description")}
                        </p>
                      </div>
                      <div className="flex flex-shrink-0 items-center gap-2">
                        <Button onClick={() => handleInstall(pack.id)} disabled={isInstalling}>
                          {isInstalling ? (
                            <>
                              <Loader2 className="mr-2 h-4 w-4 animate-spin" />
                              {t("packs.installing")}
                            </>
                          ) : (
                            t("packs.install")
                          )}
                        </Button>
                      </div>
                    </div>

                    <div className="mt-4 flex flex-wrap items-center gap-2 text-xs">
                      <span className="rounded-full bg-primary/15 px-2 py-1 text-primary">
                        {localizePackField(pack, "complexity")}
                      </span>
                      <span className="rounded-full bg-surface-elevated px-2 py-1 text-text-subtle">
                        {localizePackField(pack, "time_estimate")}
                      </span>
                      {(pack.tags ?? []).slice(0, 4).map((t) => (
                        <span key={t} className={cn("rounded-full px-2 py-1", runtimePillClass(t))}>
                          {localizePackTag(t)}
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
