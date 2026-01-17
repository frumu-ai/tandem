import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Settings } from "@/components/settings";
import { Chat } from "@/components/chat";
import { Button } from "@/components/ui";
import { useAppState } from "@/hooks/useAppState";
import {
  Settings as SettingsIcon,
  MessageSquare,
  FolderOpen,
  Sparkles,
  Shield,
} from "lucide-react";

type View = "chat" | "settings" | "onboarding";

function App() {
  const { state, loading } = useAppState();

  // Start with onboarding if no workspace, otherwise chat
  const [view, setView] = useState<View>(() => "onboarding");

  // Update view based on workspace state after loading
  const effectiveView = loading
    ? "onboarding"
    : view === "onboarding" && state?.has_workspace
      ? "chat"
      : view;

  return (
    <div className="flex h-screen bg-background">
      {/* Sidebar */}
      <motion.aside
        className="flex w-16 flex-col items-center border-r border-border bg-surface py-4"
        initial={{ x: -64 }}
        animate={{ x: 0 }}
        transition={{ duration: 0.3 }}
      >
        {/* Logo */}
        <div className="mb-8 flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-primary to-secondary">
          <Sparkles className="h-5 w-5 text-white" />
        </div>

        {/* Navigation */}
        <nav className="flex flex-1 flex-col items-center gap-2">
          <button
            onClick={() => setView("chat")}
            className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
              effectiveView === "chat"
                ? "bg-primary/20 text-primary"
                : "text-text-muted hover:bg-surface-elevated hover:text-text"
            }`}
            title="Chat"
          >
            <MessageSquare className="h-5 w-5" />
          </button>
          <button
            onClick={() => setView("settings")}
            className={`flex h-10 w-10 items-center justify-center rounded-lg transition-colors ${
              effectiveView === "settings"
                ? "bg-primary/20 text-primary"
                : "text-text-muted hover:bg-surface-elevated hover:text-text"
            }`}
            title="Settings"
          >
            <SettingsIcon className="h-5 w-5" />
          </button>
        </nav>

        {/* Security indicator */}
        <div className="mt-auto" title="Zero-trust security enabled">
          <Shield className="h-4 w-4 text-success" />
        </div>
      </motion.aside>

      {/* Main Content */}
      <main className="flex-1 overflow-hidden">
        <AnimatePresence mode="wait">
          {effectiveView === "onboarding" && !state?.has_workspace ? (
            <OnboardingView key="onboarding" onComplete={() => setView("settings")} />
          ) : effectiveView === "settings" ? (
            <Settings key="settings" onClose={() => setView("chat")} />
          ) : (
            <Chat key="chat" workspacePath={state?.workspace_path || null} />
          )}
        </AnimatePresence>
      </main>
    </div>
  );
}

interface OnboardingViewProps {
  onComplete: () => void;
}

function OnboardingView({ onComplete }: OnboardingViewProps) {
  return (
    <motion.div
      className="flex h-full flex-col items-center justify-center p-8"
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -20 }}
    >
      <div className="max-w-md text-center">
        <motion.div
          className="mx-auto mb-6 flex h-20 w-20 items-center justify-center rounded-2xl bg-gradient-to-br from-primary to-secondary"
          initial={{ scale: 0 }}
          animate={{ scale: 1 }}
          transition={{ delay: 0.2, type: "spring" }}
        >
          <Sparkles className="h-10 w-10 text-white" />
        </motion.div>

        <motion.h1
          className="mb-3 text-3xl font-bold text-text"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.3 }}
        >
          Welcome to Tandem
        </motion.h1>

        <motion.p
          className="mb-8 text-text-muted"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.4 }}
        >
          Your local-first AI workspace. Let's get started by selecting a workspace folder and
          configuring your LLM provider.
        </motion.p>

        <motion.div
          className="space-y-4"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          transition={{ delay: 0.5 }}
        >
          <div className="rounded-lg border border-border bg-surface p-4 text-left">
            <div className="flex items-start gap-3">
              <FolderOpen className="mt-0.5 h-5 w-5 text-primary" />
              <div>
                <p className="font-medium text-text">Select a workspace</p>
                <p className="text-sm text-text-muted">
                  Choose a folder where Tandem can read and write files
                </p>
              </div>
            </div>
          </div>

          <div className="rounded-lg border border-border bg-surface p-4 text-left">
            <div className="flex items-start gap-3">
              <Shield className="mt-0.5 h-5 w-5 text-success" />
              <div>
                <p className="font-medium text-text">Your data stays local</p>
                <p className="text-sm text-text-muted">
                  API keys are encrypted. No telemetry. Zero-trust security.
                </p>
              </div>
            </div>
          </div>

          <Button onClick={onComplete} size="lg" className="w-full">
            <SettingsIcon className="mr-2 h-4 w-4" />
            Open Settings
          </Button>
        </motion.div>
      </div>
    </motion.div>
  );
}

export default App;
