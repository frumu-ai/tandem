import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/geist-sans/400.css";
import "@fontsource/geist-sans/700.css";
import "@fontsource/geist-sans/900.css";
import "@fontsource-variable/geist-mono";
import App from "./App";
import "./index.css";
import "./i18n"; // Initialize i18n
import { bootstrapLanguagePreference } from "./i18n/languageSync";
import { ThemeProvider } from "@/hooks/useTheme";
import { UpdaterProvider } from "@/hooks/useUpdater";
import { MemoryIndexingProvider } from "@/contexts/MemoryIndexingContext";
import { DEFAULT_THEME_ID, getThemeById } from "@/lib/themes";
import type { ThemeId } from "@/types/theme";
import {
  applyCustomBackgroundFromMirror,
  readCustomBackgroundMirror,
} from "@/lib/customBackground";

declare global {
  interface Window {
    __tandemAppReady?: boolean;
  }
}

class AppErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { hasError: boolean; error: Error | null }
> {
  state: { hasError: boolean; error: Error | null } = { hasError: false, error: null };

  static getDerivedStateFromError(error: Error) {
    return { hasError: true, error };
  }

  componentDidCatch(error: Error) {
    console.error("[Startup] Unhandled UI error:", error);
    window.__tandemAppReady = true;
    window.dispatchEvent(new window.Event("tandem-app-ready"));
  }

  render() {
    if (this.state.hasError) {
      return (
        <div className="flex h-screen w-screen items-center justify-center app-background p-6 text-text">
          <div className="w-full max-w-lg rounded-2xl border border-border bg-surface-elevated p-6 shadow-2xl">
            <div className="mb-3 text-sm font-semibold uppercase tracking-[0.2em] text-text-subtle">
              Tandem failed to start
            </div>
            <h1 className="text-2xl font-bold text-text">Something went wrong while loading</h1>
            <p className="mt-3 text-sm leading-6 text-text-muted">
              The app hit an unexpected error before the main workspace could render. Reloading
              usually clears the issue.
            </p>
            {this.state.error ? (
              <pre className="mt-4 max-h-40 overflow-auto rounded-lg border border-border bg-surface px-4 py-3 text-xs text-text-subtle">
                {this.state.error.message}
              </pre>
            ) : null}
            <button
              type="button"
              className="mt-6 inline-flex items-center rounded-lg bg-primary px-4 py-2 font-medium text-white transition-colors hover:bg-primary-hover"
              onClick={() => window.location.reload()}
            >
              Reload app
            </button>
          </div>
        </div>
      );
    }

    return this.props.children;
  }
}

// Apply theme ASAP (before React renders) so pre-app UI and initial paint match user preference.
(() => {
  try {
    const stored = localStorage.getItem("tandem.themeId") as ThemeId | null;
    const theme = getThemeById(stored ?? DEFAULT_THEME_ID);
    for (const [name, value] of Object.entries(theme.cssVars)) {
      if (value == null) continue;
      document.documentElement.style.setProperty(name, value);
    }
    document.documentElement.dataset.theme = theme.id;

    // Apply custom background ASAP (best-effort; may be reconciled from backend after mount).
    applyCustomBackgroundFromMirror(readCustomBackgroundMirror());
  } catch {
    // ignore (e.g. storage disabled)
  }
})();

function startApp() {
  ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
    <React.StrictMode>
      <AppErrorBoundary>
        <ThemeProvider>
          <UpdaterProvider>
            <MemoryIndexingProvider>
              <App />
            </MemoryIndexingProvider>
          </UpdaterProvider>
        </ThemeProvider>
      </AppErrorBoundary>
    </React.StrictMode>
  );

  // Language sync is best-effort and must never block initial UI mount.
  void bootstrapLanguagePreference().catch(() => {
    // Continue booting with i18next default detection on any sync failure.
  });
}

startApp();
