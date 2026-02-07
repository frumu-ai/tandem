import { motion } from "framer-motion";
import { Building2, Smartphone, ExternalLink, Heart } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useUpdater } from "@/hooks/useUpdater";

export function About() {
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

  return (
    <div className="flex h-full flex-col overflow-y-auto">
      <div className="mx-auto w-full max-w-5xl p-8">
        {/* Header */}
        <motion.div
          className="mb-12 text-center"
          initial={{ opacity: 0, y: -20 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3 }}
        >
          <h1 className="mb-2 text-4xl font-bold text-text terminal-text">About</h1>
          <p className="text-text-muted">Powered by Frumu.ai</p>
        </motion.div>

        {/* GitHub Sponsors (top + centered) */}
        <motion.div
          className="mb-12 flex flex-col items-center justify-center gap-3 text-center"
          initial={{ opacity: 0, y: -10 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ duration: 0.3, delay: 0.05 }}
        >
          <button
            onClick={() => handleOpenExternal("https://github.com/sponsors/frumu-ai")}
            className="inline-flex items-center gap-2 rounded-lg bg-gradient-to-r from-pink-500/20 to-rose-500/20 px-6 py-3 text-sm font-medium text-pink-400 transition-all hover:from-pink-500/30 hover:to-rose-500/30 hover:shadow-[0_0_16px_rgba(236,72,153,0.4)]"
          >
            <Heart className="h-4 w-4 fill-current" />
            <span>Sponsor on GitHub</span>
            <ExternalLink className="h-3.5 w-3.5" />
          </button>
          <p className="text-xs text-text-subtle">
            Support the development of Tandem and other open-source projects
          </p>
        </motion.div>

        {/* Content Grid */}
        <div className="grid gap-8 md:grid-cols-2">
          {/* Frumu.ai Section */}
          <motion.div
            className="glass border-glass p-8 ring-1 ring-white/5"
            initial={{ opacity: 0, x: -20 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{ duration: 0.3, delay: 0.1 }}
          >
            <div className="mb-6 flex items-center gap-4">
              <div className="flex h-16 w-16 items-center justify-center rounded-xl bg-primary/20 text-primary">
                <Building2 className="h-8 w-8" />
              </div>
              <div>
                <h2 className="text-2xl font-bold text-text terminal-text">Frumu.ai</h2>
                <p className="text-sm text-primary">AI-Powered Development</p>
              </div>
            </div>

            <p className="mb-6 text-text-muted leading-relaxed">
              Frumu.ai builds cutting-edge AI tools that empower developers and creators. We're
              focused on making artificial intelligence accessible, powerful, and integrated
              seamlessly into your workflow.
            </p>

            <div className="space-y-3">
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-primary" />
                <p className="text-sm text-text-muted">Privacy-first AI folder tools</p>
              </div>
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-primary" />
                <p className="text-sm text-text-muted">Local-first architecture</p>
              </div>
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-primary" />
                <p className="text-sm text-text-muted">Open-source friendly</p>
              </div>
            </div>

            <button
              onClick={() => handleOpenExternal("https://frumu.ai/")}
              className="mt-8 flex w-full items-center justify-center gap-2 rounded-lg bg-primary/20 px-6 py-3 text-primary transition-all hover:bg-primary/30 hover:shadow-[0_0_12px_rgba(59,130,246,0.45)]"
            >
              <span className="font-medium">Visit Frumu.ai</span>
              <ExternalLink className="h-4 w-4" />
            </button>
          </motion.div>

          {/* AIMajin Section */}
          <motion.div
            className="glass border-glass p-8 ring-1 ring-white/5"
            initial={{ opacity: 0, x: 20 }}
            animate={{ opacity: 1, x: 0 }}
            transition={{ duration: 0.3, delay: 0.2 }}
          >
            <div className="mb-6 flex items-center gap-4">
              <div className="flex h-16 w-16 items-center justify-center rounded-xl bg-secondary/20 text-secondary">
                <Smartphone className="h-8 w-8" />
              </div>
              <div>
                <h2 className="text-2xl font-bold text-text terminal-text">AIMajin</h2>
                <p className="text-sm text-secondary">Mobile AI Generation</p>
              </div>
            </div>

            <p className="mb-6 text-text-muted leading-relaxed">
              AIMajin is our mobile AI generation app that puts powerful AI capabilities right in
              your pocket. Create, generate, and explore AI-powered content anywhere, anytime.
            </p>

            <div className="space-y-3">
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-secondary" />
                <p className="text-sm text-text-muted">AI image generation on mobile</p>
              </div>
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-secondary" />
                <p className="text-sm text-text-muted">Intuitive mobile-first interface</p>
              </div>
              <div className="flex items-start gap-3">
                <div className="mt-1 h-2 w-2 rounded-full bg-secondary" />
                <p className="text-sm text-text-muted">Create on the go</p>
              </div>
            </div>

            <button
              onClick={() => handleOpenExternal("https://aimajin.com/")}
              className="mt-8 flex w-full items-center justify-center gap-2 rounded-lg bg-secondary/20 px-6 py-3 text-secondary transition-all hover:bg-secondary/30 hover:shadow-[0_0_12px_rgba(168,85,247,0.45)]"
            >
              <span className="font-medium">Check out AIMajin</span>
              <ExternalLink className="h-4 w-4" />
            </button>
          </motion.div>
        </div>

        {/* Footer */}
        <motion.div
          className="mt-12 border-t border-border pt-8 text-center"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ duration: 0.3, delay: 0.3 }}
        >
          <p className="text-sm text-text-subtle">Tandem is built with ❤️ by the Frumu.ai team</p>
          <p className="mt-2 text-xs text-text-subtle">
            Open source • Privacy-focused • For everyone
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
              {updateStatus === "checking" && "Checking for updates..."}
              {updateStatus === "downloading" &&
                `Downloading update${updateProgress ? ` (${Math.round(updateProgress.percent)}%)` : ""}...`}
              {updateStatus === "installing" && "Installing update..."}
              {updateStatus === "available" && "Install update"}
              {(updateStatus === "idle" ||
                updateStatus === "upToDate" ||
                updateStatus === "installed" ||
                updateStatus === "error") &&
                "Check for updates"}
            </button>
            <p className="text-xs text-text-subtle">
              {updateStatus === "available" && updateInfo
                ? `Update available: v${updateInfo.version}`
                : updateStatus === "upToDate"
                  ? "You're on the latest version."
                  : updateStatus === "installed"
                    ? "Update installed. Relaunching..."
                    : updateStatus === "error"
                      ? updateError || "Update check failed."
                      : ""}
            </p>
          </div>
        </motion.div>
      </div>
    </div>
  );
}
