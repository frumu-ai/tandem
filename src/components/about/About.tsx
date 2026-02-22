import { motion } from "framer-motion";
import { BookOpen, Server, ExternalLink, Heart } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useUpdater } from "@/hooks/useUpdater";
import { useTranslation } from "react-i18next";

export function About() {
  const { t } = useTranslation(["common", "settings"]);
  const {
    status: updateStatus,
    updateInfo,
    error: updateError,
    progress: updateProgress,
    checkUpdates,
    installUpdate,
  } = useUpdater();

  const handleOpenExternal = async (url: string) => {
    try {
      await openUrl(url);
    } catch (error) {
      console.error("Failed to open URL:", error);
    }
  };

  const downloadingLabel = updateProgress
    ? t("aboutPage.downloadingWithProgress", {
        ns: "common",
        percent: Math.round(updateProgress.percent),
      })
    : t("updates.downloadingUpdate", { ns: "settings" });

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <div className="mx-auto w-full max-w-5xl p-8">
        <motion.div
          className="mb-12 text-center"
          initial={{ opacity: 0, y: -20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3 }}
        >
          <h1 className="mb-2 text-4xl font-bold text-text terminal-text">
            {t("aboutPage.title", { ns: "common" })}
          </h1>
          <p className="text-text-muted">{t("aboutPage.poweredBy", { ns: "common" })}</p>
        </motion.div>

        <motion.div
          className="mb-12 flex flex-col items-center justify-center gap-3 text-center"
          initial={{ opacity: 0, y: -10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, delay: 0.05 }}
        >
          <button
            onClick={() => handleOpenExternal("https://github.com/sponsors/frumu-ai")}
            className="inline-flex items-center gap-3 rounded-lg bg-gradient-to-r from-pink-500/20 to-rose-500/20 px-6 py-3 text-sm font-medium text-pink-400 transition-all hover:from-pink-500/30 hover:to-rose-500/30 hover:shadow-[0_0_16px_rgba(236,72,153,0.4)]"
          >
            <Heart className="h-4 w-4 fill-current" />
            <span className="tracking-wide">{t("aboutPage.sponsorCta", { ns: "common" })}</span>
            <ExternalLink className="h-3.5 w-3.5" />
          </button>
          <p className="text-xs text-text-subtle">
            {t("aboutPage.sponsorSubtitle", { ns: "common" })}
          </p>
        </motion.div>

        <div className="grid gap-8 md:grid-cols-2">
          <motion.div
            className="glass border-glass flex h-full flex-col p-8 ring-1 ring-white/5"
            initial={{ opacity: 0, x: -20 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{ duration: 0.3, delay: 0.1 }}
          >
            <div className="mb-6 flex items-center gap-4">
              <div className="flex h-16 w-16 items-center justify-center rounded-xl bg-primary/20 text-primary">
                <BookOpen className="h-8 w-8" />
              </div>
              <div>
                <h2 className="text-2xl font-bold text-text terminal-text">Documentation</h2>
                <p className="text-sm text-primary">
                  {t("aboutPage.frumu.tagline", { ns: "common" })}
                </p>
              </div>
            </div>

            <p className="mb-6 text-text-muted leading-relaxed">
              {t("aboutPage.frumu.description", { ns: "common" })}
            </p>

            <div className="space-y-3">
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-primary" />
                <p className="text-sm text-text-muted">
                  {t("aboutPage.frumu.bullet1", { ns: "common" })}
                </p>
              </div>
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-primary" />
                <p className="text-sm text-text-muted">
                  {t("aboutPage.frumu.bullet2", { ns: "common" })}
                </p>
              </div>
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-primary" />
                <p className="text-sm text-text-muted">
                  {t("aboutPage.frumu.bullet3", { ns: "common" })}
                </p>
              </div>
            </div>

            <div className="mt-auto pt-6">
              <button
                onClick={() => handleOpenExternal("https://tandem.frumu.ai/docs/")}
                className="flex w-full items-center justify-center gap-3 rounded-lg bg-primary/20 px-6 py-3 text-primary transition-all hover:bg-primary/30 hover:shadow-[0_0_12px_rgba(59,130,246,0.45)]"
              >
                <span className="font-medium tracking-wide">
                  {t("aboutPage.frumu.cta", { ns: "common" })}
                </span>
                <ExternalLink className="h-4 w-4" />
              </button>
            </div>
          </motion.div>

          <motion.div
            className="glass border-glass flex h-full flex-col p-8 ring-1 ring-white/5"
            initial={{ opacity: 0, x: 20 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{ duration: 0.3, delay: 0.2 }}
          >
            <div className="mb-6 flex items-center gap-4">
              <div className="flex h-16 w-16 items-center justify-center rounded-xl bg-secondary/20 text-secondary">
                <Server className="h-8 w-8" />
              </div>
              <div>
                <h2 className="text-2xl font-bold text-text terminal-text">Examples</h2>
                <p className="text-sm text-secondary">
                  {t("aboutPage.aimajin.tagline", { ns: "common" })}
                </p>
              </div>
            </div>

            <p className="mb-6 text-text-muted leading-relaxed">
              {t("aboutPage.aimajin.description", { ns: "common" })}
            </p>

            <div className="space-y-3">
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-secondary" />
                <p className="text-sm text-text-muted">
                  {t("aboutPage.aimajin.bullet1", { ns: "common" })}
                </p>
              </div>
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-secondary" />
                <p className="text-sm text-text-muted">
                  {t("aboutPage.aimajin.bullet2", { ns: "common" })}
                </p>
              </div>
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-secondary" />
                <p className="text-sm text-text-muted">
                  {t("aboutPage.aimajin.bullet3", { ns: "common" })}
                </p>
              </div>
            </div>

            <div className="mt-auto pt-6">
              <button
                onClick={() =>
                  handleOpenExternal(
                    "https://github.com/frumu-ai/tandem/tree/main/examples/vps-web-portal"
                  )
                }
                className="flex w-full items-center justify-center gap-3 rounded-lg bg-secondary/20 px-6 py-3 text-secondary transition-all hover:bg-secondary/30 hover:shadow-[0_0_12px_rgba(168,85,247,0.45)]"
              >
                <span className="font-medium tracking-wide">
                  {t("aboutPage.aimajin.cta", { ns: "common" })}
                </span>
                <ExternalLink className="h-4 w-4" />
              </button>
            </div>
          </motion.div>
        </div>

        <motion.div
          className="mt-12 border-t border-border pt-8 text-center"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.3, delay: 0.3 }}
        >
          <p className="text-sm text-text-subtle">
            {t("aboutPage.footerBuiltBy", { ns: "common" })}
          </p>
          <p className="mt-2 text-xs text-text-subtle">
            {t("aboutPage.footerValues", { ns: "common" })}
          </p>
          <div className="mt-6 flex flex-col items-center gap-3">
            <button
              onClick={updateStatus === "available" ? installUpdate : () => checkUpdates(false)}
              disabled={
                updateStatus === "checking" ||
                updateStatus === "downloading" ||
                updateStatus === "installing"
              }
              className="rounded-lg border border-border px-4 py-2 text-sm text-text transition-all hover:border-primary hover:text-primary disabled:cursor-not-allowed disabled:opacity-60"
            >
              {updateStatus === "checking" && t("updates.checkingForUpdates", { ns: "settings" })}
              {updateStatus === "downloading" && downloadingLabel}
              {updateStatus === "installing" && t("updates.installingUpdate", { ns: "settings" })}
              {updateStatus === "available" && t("aboutPage.installUpdate", { ns: "common" })}
              {(updateStatus === "idle" ||
                updateStatus === "upToDate" ||
                updateStatus === "installed" ||
                updateStatus === "error") &&
                t("updates.checkForUpdates", { ns: "settings" })}
            </button>
            <p className="text-xs text-text-subtle">
              {updateStatus === "available" && updateInfo
                ? t("updates.updateAvailable", { ns: "settings", version: updateInfo.version })
                : updateStatus === "upToDate"
                  ? t("updates.upToDate", { ns: "settings" })
                  : updateStatus === "installed"
                    ? t("updates.installedRelaunching", { ns: "settings" })
                    : updateStatus === "error"
                      ? updateError || t("aboutPage.updateCheckFailed", { ns: "common" })
                      : ""}
            </p>
            {updateStatus === "available" &&
            updateInfo &&
            typeof updateInfo.body === "string" &&
            updateInfo.body.trim().length > 0 ? (
              <div className="w-full max-w-2xl space-y-1 rounded-lg border border-border bg-surface-elevated/30 p-3 text-left">
                <div className="text-[11px] font-semibold uppercase tracking-wide text-text-subtle">
                  What&apos;s New
                </div>
                <div className="max-h-40 overflow-y-auto whitespace-pre-wrap text-xs text-text-subtle">
                  {updateInfo.body.trim()}
                </div>
              </div>
            ) : null}
          </div>
        </motion.div>
      </div>
    </div>
  );
}
