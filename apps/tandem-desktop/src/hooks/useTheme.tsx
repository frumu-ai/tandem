import React, { createContext, useCallback, useContext, useEffect, useMemo, useState } from "react";
import type { ThemeDefinition, ThemeId } from "@/types/theme";
import { DEFAULT_THEME_ID, THEMES, cycleThemeId, getThemeById } from "@/lib/themes";
import { getCustomBackground, getUserTheme, setUserTheme } from "@/lib/tauri";
import {
  applyCustomBackground,
  applyCustomBackgroundUrl,
  getCustomBackgroundAssetUrl,
  mirrorCustomBackgroundToLocalStorage,
  CUSTOM_BG_STORAGE_KEY,
  tryReadCustomBackgroundDataUrl,
} from "@/lib/customBackground";

const THEME_STORAGE_KEY = "tandem.themeId";

type ThemeContextValue = {
  themeId: ThemeId;
  theme: ThemeDefinition;
  availableThemes: ThemeDefinition[];
  setThemeId: (id: ThemeId) => Promise<void>;
  cycleTheme: () => Promise<void>;
  isLoaded: boolean;
};

const ThemeContext = createContext<ThemeContextValue | null>(null);

function applyCssVars(theme: ThemeDefinition) {
  const root = document.documentElement;
  for (const [name, value] of Object.entries(theme.cssVars)) {
    if (value == null) continue;
    root.style.setProperty(name, value);
  }
  root.dataset.theme = theme.id;
}

function preloadImage(src: string, timeoutMs: number): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    let settled = false;
    const img = new Image();

    const done = (ok: boolean) => {
      if (settled) return;
      settled = true;
      resolve(ok);
    };

    const t = window.setTimeout(() => done(false), timeoutMs);
    img.onload = () => {
      window.clearTimeout(t);
      done(true);
    };
    img.onerror = () => {
      window.clearTimeout(t);
      done(false);
    };
    img.src = src;
  });
}

export function ThemeProvider({ children }: { children: React.ReactNode }) {
  const [themeId, setThemeIdState] = useState<ThemeId>(DEFAULT_THEME_ID);
  const [isLoaded, setIsLoaded] = useState(false);

  // Load persisted theme (best-effort; supports web dev as well)
  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        const persisted = await getUserTheme();
        const next = (persisted as ThemeId) || DEFAULT_THEME_ID;
        if (!cancelled) {
          setThemeIdState(next);
          applyCssVars(getThemeById(next));
          try {
            localStorage.setItem(THEME_STORAGE_KEY, next);
          } catch {
            // ignore storage failures
          }
        }
      } catch {
        if (!cancelled) {
          // fallback to default theme already set
          applyCssVars(getThemeById(DEFAULT_THEME_ID));
        }
      } finally {
        if (!cancelled) setIsLoaded(true);
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  // Load persisted custom background (best-effort; supports web dev as well)
  useEffect(() => {
    let cancelled = false;

    (async () => {
      try {
        const info = await getCustomBackground();
        if (cancelled) return;
        applyCustomBackground(info);
        mirrorCustomBackgroundToLocalStorage(info);

        // Fallback: in some packaged builds, `asset:` URLs can fail to load even when the file exists.
        // If the asset URL doesn't load quickly, replace it with a data URL read from disk.
        const assetUrl = getCustomBackgroundAssetUrl(info);
        if (info?.settings?.enabled && info.file_path && assetUrl) {
          const ok = await preloadImage(assetUrl, 1200);
          if (!ok) {
            const dataUrl = await tryReadCustomBackgroundDataUrl(info.file_path);
            if (dataUrl && !cancelled) {
              applyCustomBackgroundUrl(info.settings, dataUrl);
            }
          }
        }
      } catch {
        if (cancelled) return;
        applyCustomBackground(null);
        try {
          localStorage.removeItem(CUSTOM_BG_STORAGE_KEY);
        } catch {
          // ignore storage failures
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, []);

  const theme = useMemo(() => getThemeById(themeId), [themeId]);

  // Apply whenever theme changes (including changes from UI)
  useEffect(() => {
    applyCssVars(theme);
  }, [theme]);

  const setThemeId = useCallback(async (id: ThemeId) => {
    setThemeIdState(id);
    try {
      localStorage.setItem(THEME_STORAGE_KEY, id);
    } catch {
      // ignore storage failures
    }
    try {
      await setUserTheme(id);
    } catch {
      // ignore persistence failures (e.g. web dev)
    }
  }, []);

  const cycleTheme = useCallback(async () => {
    const next = cycleThemeId(themeId);
    await setThemeId(next);
  }, [setThemeId, themeId]);

  const value = useMemo<ThemeContextValue>(
    () => ({
      themeId,
      theme,
      availableThemes: THEMES,
      setThemeId,
      cycleTheme,
      isLoaded,
    }),
    [cycleTheme, isLoaded, setThemeId, theme, themeId]
  );

  return <ThemeContext.Provider value={value}>{children}</ThemeContext.Provider>;
}

export function useTheme(): ThemeContextValue {
  const ctx = useContext(ThemeContext);
  if (!ctx) throw new Error("useTheme must be used within ThemeProvider");
  return ctx;
}
