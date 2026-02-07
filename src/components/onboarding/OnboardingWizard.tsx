import { motion } from "framer-motion";
import {
  ArrowRight,
  CheckCircle,
  FolderOpen,
  MessageSquare,
  Settings as SettingsIcon,
  Sparkles,
} from "lucide-react";
import { Button } from "@/components/ui/Button";
import { cn } from "@/lib/utils";

interface OnboardingWizardProps {
  hasWorkspace: boolean;
  hasConfiguredProvider: boolean;
  error?: string | null;
  onChooseFolder: () => void;
  onOpenProviders: () => void;
  onBrowsePacks: () => void;
  onSkip: () => void;
}

export function OnboardingWizard({
  hasWorkspace,
  hasConfiguredProvider,
  error,
  onChooseFolder,
  onOpenProviders,
  onBrowsePacks,
  onSkip,
}: OnboardingWizardProps) {
  return (
    <motion.div
      className="flex h-full w-full flex-col items-center justify-center p-8"
      initial={{ opacity: 0, y: 40 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -40 }}
      transition={{ duration: 0.5, ease: "easeOut" }}
    >
      <div className="w-full max-w-xl space-y-6">
        <div className="text-center">
          <h1 className="mb-2 text-3xl font-bold text-text">Welcome to Tandem</h1>
          <p className="text-text-muted">
            Let’s get you to a first outcome quickly — pick a folder, connect AI, then run a guided
            starter pack.
          </p>
        </div>

        {error && (
          <div className="rounded-lg border border-error/20 bg-error/10 p-4 text-sm text-error">
            {error}
          </div>
        )}

        <div className="space-y-3">
          <div
            className={cn(
              "rounded-xl border p-4",
              hasWorkspace ? "border-border bg-surface" : "border-primary/40 bg-primary/5"
            )}
          >
            <div className="flex items-start gap-3">
              <FolderOpen
                className={cn("mt-0.5 h-5 w-5", hasWorkspace ? "text-success" : "text-primary")}
              />
              <div className="flex-1">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <p className="font-medium text-text">
                      1) Choose a folder{" "}
                      {hasWorkspace && (
                        <span className="ml-2 inline-flex items-center gap-1 text-xs text-success">
                          <CheckCircle className="h-3.5 w-3.5" /> Done
                        </span>
                      )}
                    </p>
                    <p className="text-sm text-text-muted">
                      Tandem only works inside folders you choose.
                    </p>
                  </div>
                  <Button onClick={onChooseFolder} disabled={hasWorkspace}>
                    {hasWorkspace ? "Selected" : "Pick Folder"}
                  </Button>
                </div>
              </div>
            </div>
          </div>

          <div
            className={cn(
              "rounded-xl border p-4",
              hasConfiguredProvider ? "border-border bg-surface" : "border-primary/40 bg-primary/5"
            )}
          >
            <div className="flex items-start gap-3">
              <MessageSquare
                className={cn(
                  "mt-0.5 h-5 w-5",
                  hasConfiguredProvider ? "text-success" : "text-primary"
                )}
              />
              <div className="flex-1">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <p className="font-medium text-text">
                      2) Connect AI{" "}
                      {hasConfiguredProvider && (
                        <span className="ml-2 inline-flex items-center gap-1 text-xs text-success">
                          <CheckCircle className="h-3.5 w-3.5" /> Connected
                        </span>
                      )}
                    </p>
                    <p className="text-sm text-text-muted">
                      Use OpenAI, Anthropic, OpenRouter, or a local model.
                    </p>
                  </div>
                  <Button onClick={onOpenProviders} variant="ghost">
                    <SettingsIcon className="mr-2 h-4 w-4" />
                    Providers
                  </Button>
                </div>
              </div>
            </div>
          </div>

          <div className="rounded-xl border border-border bg-surface p-4">
            <div className="flex items-start gap-3">
              <Sparkles className="mt-0.5 h-5 w-5 text-accent" />
              <div className="flex-1">
                <div className="flex items-center justify-between gap-3">
                  <div>
                    <p className="font-medium text-text">3) Run a starter pack</p>
                    <p className="text-sm text-text-muted">
                      We’ll set up a sample folder and draft the first message for you.
                    </p>
                  </div>
                  <Button onClick={onBrowsePacks}>
                    Browse Packs <ArrowRight className="ml-2 h-4 w-4" />
                  </Button>
                </div>
              </div>
            </div>
          </div>

          <div className="flex items-center justify-center">
            <button onClick={onSkip} className="text-sm text-text-subtle hover:text-text">
              Skip for now — I’ll explore on my own
            </button>
          </div>
        </div>
      </div>
    </motion.div>
  );
}
