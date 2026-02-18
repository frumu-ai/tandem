import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { Blocks } from "lucide-react";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/Button";
import { SkillsTab } from "./SkillsTab";
import { PluginsTab } from "./PluginsTab";
import { IntegrationsTab } from "./IntegrationsTab";
import { ModesTab } from "./ModesTab";

export type ExtensionsTabId = "skills" | "plugins" | "mcp" | "modes";

interface ExtensionsProps {
  workspacePath?: string | null;
  onClose?: () => void;
  initialTab?: ExtensionsTabId;
  onInitialTabConsumed?: () => void;
  onStartModeBuilderChat?: (seedPrompt: string) => void;
}

export function Extensions({
  workspacePath,
  onClose,
  initialTab,
  onInitialTabConsumed,
  onStartModeBuilderChat,
}: ExtensionsProps) {
  const { t } = useTranslation("common");
  const [activeTab, setActiveTab] = useState<ExtensionsTabId>(() => initialTab ?? "skills");

  useEffect(() => {
    if (!initialTab) return;
    onInitialTabConsumed?.();
  }, [initialTab, onInitialTabConsumed]);

  return (
    <motion.div
      className="h-full overflow-y-auto p-6"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.3 }}
    >
      <div className="mx-auto max-w-3xl space-y-8">
        {/* Header */}
        <div className="flex items-center justify-between">
          <div className="flex items-center gap-3">
            <div className="flex h-12 w-12 items-center justify-center rounded-xl bg-primary/10">
              <Blocks className="h-6 w-6 text-primary" />
            </div>
            <div>
              <h1 className="text-2xl font-bold text-text">{t("extensions.title")}</h1>
              <p className="text-text-muted">{t("extensions.subtitle")}</p>
            </div>
          </div>
          {onClose && (
            <Button variant="ghost" onClick={onClose}>
              {t("actions.close")}
            </Button>
          )}
        </div>

        {/* Tabs */}
        <div className="rounded-lg border border-border bg-surface">
          <div className="flex border-b border-border">
            <button
              type="button"
              onClick={() => setActiveTab("skills")}
              className={cn(
                "flex-1 px-4 py-3 text-sm font-medium transition-colors flex items-center justify-center",
                activeTab === "skills"
                  ? "border-b-2 border-primary text-primary"
                  : "text-text-muted hover:text-text hover:bg-surface-elevated"
              )}
            >
              {t("extensions.tabs.skills")}
            </button>
            <button
              type="button"
              onClick={() => setActiveTab("plugins")}
              className={cn(
                "flex-1 px-4 py-3 text-sm font-medium transition-colors flex items-center justify-center",
                activeTab === "plugins"
                  ? "border-b-2 border-primary text-primary"
                  : "text-text-muted hover:text-text hover:bg-surface-elevated"
              )}
            >
              {t("extensions.tabs.plugins")}
            </button>
            <button
              type="button"
              onClick={() => setActiveTab("mcp")}
              className={cn(
                "flex-1 px-4 py-3 text-sm font-medium transition-colors flex items-center justify-center",
                activeTab === "mcp"
                  ? "border-b-2 border-primary text-primary"
                  : "text-text-muted hover:text-text hover:bg-surface-elevated"
              )}
            >
              {t("extensions.tabs.mcp")}
            </button>
            <button
              type="button"
              onClick={() => setActiveTab("modes")}
              className={cn(
                "flex-1 px-4 py-3 text-sm font-medium transition-colors flex items-center justify-center",
                activeTab === "modes"
                  ? "border-b-2 border-primary text-primary"
                  : "text-text-muted hover:text-text hover:bg-surface-elevated"
              )}
            >
              {t("extensions.tabs.modes")}
            </button>
          </div>

          <div className="p-6">
            {activeTab === "skills" ? (
              <SkillsTab workspacePath={workspacePath ?? null} />
            ) : activeTab === "plugins" ? (
              <PluginsTab workspacePath={workspacePath ?? null} />
            ) : activeTab === "mcp" ? (
              <IntegrationsTab workspacePath={workspacePath ?? null} />
            ) : (
              <ModesTab
                workspacePath={workspacePath ?? null}
                onStartModeBuilderChat={onStartModeBuilderChat}
              />
            )}
          </div>
        </div>
      </div>
    </motion.div>
  );
}
