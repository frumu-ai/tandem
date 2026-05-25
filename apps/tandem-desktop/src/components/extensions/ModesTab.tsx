import { useState } from "react";
import { Lightbulb } from "lucide-react";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/Card";
import { ModesSettings } from "@/components/settings/ModesSettings";
import { ModeBuilderWizard } from "@/components/settings/modes/ModeBuilderWizard";
import { cn } from "@/lib/utils";

interface ModesTabProps {
  workspacePath?: string | null;
  onStartModeBuilderChat?: (seedPrompt: string) => void;
}

export function ModesTab({ workspacePath, onStartModeBuilderChat }: ModesTabProps) {
  const [activeView, setActiveView] = useState<"guided" | "advanced">("guided");

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <div className="flex items-start gap-3">
            <Lightbulb className="mt-0.5 h-5 w-5 text-primary" />
            <div>
              <CardTitle>Modes</CardTitle>
              <CardDescription>
                Advanced feature: create reusable mode profiles that control behavior, tool access,
                and edit boundaries.
              </CardDescription>
            </div>
          </div>
        </CardHeader>
        <CardContent className="space-y-2 text-sm text-text-muted">
          <p>
            Start with one mode based on <span className="font-mono">ask</span> or{" "}
            <span className="font-mono">coder</span>, then optionally add{" "}
            <span className="font-mono">allowed_tools</span> and{" "}
            <span className="font-mono">edit_globs</span>.
          </p>
          <p>
            If a mode is invalid or missing, Tandem safely falls back to{" "}
            <span className="font-mono">ask</span>.
          </p>
        </CardContent>
      </Card>

      <Card className="p-0">
        <div className="flex border-b border-border">
          <button
            type="button"
            onClick={() => setActiveView("guided")}
            className={cn(
              "flex-1 px-4 py-3 text-sm font-medium transition-colors",
              activeView === "guided"
                ? "border-b-2 border-primary text-primary"
                : "text-text-muted hover:text-text hover:bg-surface-elevated"
            )}
          >
            Guided Builder
          </button>
          <button
            type="button"
            onClick={() => setActiveView("advanced")}
            className={cn(
              "flex-1 px-4 py-3 text-sm font-medium transition-colors",
              activeView === "advanced"
                ? "border-b-2 border-primary text-primary"
                : "text-text-muted hover:text-text hover:bg-surface-elevated"
            )}
          >
            Advanced Editor
          </button>
        </div>
      </Card>

      {activeView === "guided" ? (
        <ModeBuilderWizard
          workspacePath={workspacePath}
          onStartModeBuilderChat={onStartModeBuilderChat}
        />
      ) : (
        <Card>
          <CardHeader>
            <CardTitle>Advanced Editor</CardTitle>
            <CardDescription>
              Prefer direct field control? Use the advanced editor for full manual mode CRUD and
              JSON import/export.
            </CardDescription>
          </CardHeader>
          <CardContent>
            <ModesSettings />
          </CardContent>
        </Card>
      )}
    </div>
  );
}
