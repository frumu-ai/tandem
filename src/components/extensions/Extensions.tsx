import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { Blocks } from "lucide-react";
import { cn } from "@/lib/utils";
import { Button } from "@/components/ui/Button";
import { SkillsTab } from "./SkillsTab";
import { PluginsTab } from "./PluginsTab";
import { IntegrationsTab } from "./IntegrationsTab";

export type ExtensionsTabId = "skills" | "plugins" | "integrations";

interface ExtensionsProps {
  workspacePath?: string | null;
  onClose?: () => void;
  initialTab?: ExtensionsTabId;
  onInitialTabConsumed?: () => void;
}

export function Extensions({
  workspacePath,
  onClose,
  initialTab,
  onInitialTabConsumed,
}: ExtensionsProps) {
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
              <h1 className="text-2xl font-bold text-text">Extensions</h1>
              <p className="text-text-muted">Manage skills, plugins, and MCP integrations</p>
            </div>
          </div>
          {onClose && (
            <Button variant="ghost" onClick={onClose}>
              Close
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
              Skills
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
              Plugins
            </button>
            <button
              type="button"
              onClick={() => setActiveTab("integrations")}
              className={cn(
                "flex-1 px-4 py-3 text-sm font-medium transition-colors flex items-center justify-center",
                activeTab === "integrations"
                  ? "border-b-2 border-primary text-primary"
                  : "text-text-muted hover:text-text hover:bg-surface-elevated"
              )}
            >
              Integrations (MCP)
            </button>
          </div>

          <div className="p-6">
            {activeTab === "skills" ? (
              <SkillsTab workspacePath={workspacePath ?? null} />
            ) : activeTab === "plugins" ? (
              <PluginsTab workspacePath={workspacePath ?? null} />
            ) : (
              <IntegrationsTab workspacePath={workspacePath ?? null} />
            )}
          </div>
        </div>
      </div>
    </motion.div>
  );
}
