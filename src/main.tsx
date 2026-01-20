import React from "react";
import ReactDOM from "react-dom/client";
import "@fontsource/geist-sans/400.css";
import "@fontsource/geist-sans/700.css";
import "@fontsource/geist-sans/900.css";
import "@fontsource-variable/geist-mono";
import App from "./App";
import "./index.css";
import { ThemeProvider } from "@/hooks/useTheme";
import { UpdaterProvider } from "@/hooks/useUpdater";
import { DEFAULT_THEME_ID, getThemeById } from "@/lib/themes";
import type { ThemeId } from "@/types/theme";

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
  } catch {
    // ignore (e.g. storage disabled)
  }
})();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <UpdaterProvider>
        <App />
      </UpdaterProvider>
    </ThemeProvider>
  </React.StrictMode>
);
