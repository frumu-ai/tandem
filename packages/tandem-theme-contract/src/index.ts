export type ThemeId =
  | "charcoal_fire"
  | "electric_blue"
  | "emerald_night"
  | "hello_bunny"
  | "porcelain"
  | "neon_riot"
  | "cosmic_glass"
  | "pink_pony"
  | "zen_dusk";

export type ThemeDefinition = {
  id: ThemeId;
  name: string;
  description: string;
  cssVars: Record<string, string>;
};

export const DEFAULT_THEME_ID: ThemeId = "electric_blue";

export const STRUCTURAL_THEME_VARS: Record<string, string> = {
  "--radius": "0px",
  "--border-width": "1px",
  "--border-width-strong": "2px",
  "--shadow-offset": "2px 2px 0 rgba(0, 0, 0, 0.32)",
  "--shadow-offset-lg": "4px 4px 0 rgba(0, 0, 0, 0.32)",
  "--clip-primary-cta": "polygon(10% 0, 100% 0, 100% 100%, 0 100%, 0 25%)",
};

export const THEMES: ThemeDefinition[] = [
  {
    id: "charcoal_fire",
    name: "Charcoal & Fire",
    description:
      "Deep charcoal surfaces with solar-yellow power accents and crimson security cues.",
    cssVars: {
      "--color-background": "#121212",
      "--color-surface": "#141414",
      "--color-surface-elevated": "#1a1a1a",
      "--color-border": "rgba(245, 245, 245, 0.10)",
      "--color-border-subtle": "rgba(245, 245, 245, 0.06)",
      "--color-primary": "#F59E0B",
      "--color-primary-hover": "#D97706",
      "--color-primary-muted": "#B45309",
      "--color-secondary": "#EF4444",
      "--color-secondary-hover": "#DC2626",
      "--color-success": "#10B981",
      "--color-warning": "#F59E0B",
      "--color-error": "#EF4444",
      "--color-text": "#F5F5F5",
      "--color-text-muted": "rgba(245, 245, 245, 0.70)",
      "--color-text-subtle": "rgba(245, 245, 245, 0.50)",
      "--color-glass": "rgba(255, 255, 255, 0.03)",
      "--color-glass-border": "rgba(255, 255, 255, 0.08)",
      "--font-sans": '"Manrope", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(245, 158, 11, 0.16)",
      "--tcp-glow-b": "rgba(239, 68, 68, 0.12)",
    },
  },
  {
    id: "electric_blue",
    name: "Electric Blue",
    description: "The original Tandem look: electric-blue primary with purple secondary.",
    cssVars: {
      "--color-background": "#0a0a0f",
      "--color-surface": "#12121a",
      "--color-surface-elevated": "#1a1a24",
      "--color-border": "#2a2a3a",
      "--color-border-subtle": "#1f1f2e",
      "--color-primary": "#3b82f6",
      "--color-primary-hover": "#2563eb",
      "--color-primary-muted": "#1d4ed8",
      "--color-secondary": "#8b5cf6",
      "--color-secondary-hover": "#7c3aed",
      "--color-success": "#10b981",
      "--color-warning": "#f59e0b",
      "--color-error": "#ef4444",
      "--color-text": "#f8fafc",
      "--color-text-muted": "#94a3b8",
      "--color-text-subtle": "#64748b",
      "--color-glass": "rgba(18, 18, 26, 0.8)",
      "--color-glass-border": "rgba(255, 255, 255, 0.1)",
      "--font-sans": '"Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(59, 130, 246, 0.16)",
      "--tcp-glow-b": "rgba(139, 92, 246, 0.12)",
    },
  },
  {
    id: "emerald_night",
    name: "Emerald Night",
    description: "Dark glass with emerald primary and cyan secondary highlights.",
    cssVars: {
      "--color-background": "#0b1010",
      "--color-surface": "#0f1616",
      "--color-surface-elevated": "#142020",
      "--color-border": "rgba(226, 232, 240, 0.12)",
      "--color-border-subtle": "rgba(226, 232, 240, 0.08)",
      "--color-primary": "#10B981",
      "--color-primary-hover": "#059669",
      "--color-primary-muted": "#047857",
      "--color-secondary": "#22D3EE",
      "--color-secondary-hover": "#06B6D4",
      "--color-success": "#22C55E",
      "--color-warning": "#F59E0B",
      "--color-error": "#EF4444",
      "--color-text": "#F1F5F9",
      "--color-text-muted": "rgba(241, 245, 249, 0.72)",
      "--color-text-subtle": "rgba(241, 245, 249, 0.52)",
      "--color-glass": "rgba(15, 22, 22, 0.75)",
      "--color-glass-border": "rgba(255, 255, 255, 0.10)",
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(16, 185, 129, 0.16)",
      "--tcp-glow-b": "rgba(34, 211, 238, 0.12)",
    },
  },
  {
    id: "hello_bunny",
    name: "Hello Bunny",
    description: "Soft pink glass with berry accents and a cozy, playful vibe.",
    cssVars: {
      "--color-background": "#140A12",
      "--color-surface": "#1C0E1A",
      "--color-surface-elevated": "#251022",
      "--color-border": "rgba(255, 228, 242, 0.12)",
      "--color-border-subtle": "rgba(255, 228, 242, 0.08)",
      "--color-primary": "#FB7185",
      "--color-primary-hover": "#F43F5E",
      "--color-primary-muted": "#E11D48",
      "--color-secondary": "#C084FC",
      "--color-secondary-hover": "#A855F7",
      "--color-success": "#34D399",
      "--color-warning": "#FBBF24",
      "--color-error": "#FB7185",
      "--color-text": "#FFEAF4",
      "--color-text-muted": "rgba(255, 234, 244, 0.74)",
      "--color-text-subtle": "rgba(255, 234, 244, 0.52)",
      "--color-glass": "rgba(255, 255, 255, 0.04)",
      "--color-glass-border": "rgba(255, 228, 242, 0.10)",
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(251, 113, 133, 0.16)",
      "--tcp-glow-b": "rgba(192, 132, 252, 0.12)",
    },
  },
  {
    id: "porcelain",
    name: "Porcelain",
    description: "Plain, bright whites with soft pastel accents and glassy structure.",
    cssVars: {
      "--color-background": "#F8FAFC",
      "--color-surface": "#FFFFFF",
      "--color-surface-elevated": "#F1F5F9",
      "--color-border": "rgba(15, 23, 42, 0.12)",
      "--color-border-subtle": "rgba(15, 23, 42, 0.08)",
      "--color-primary": "#6366F1",
      "--color-primary-hover": "#4F46E5",
      "--color-primary-muted": "#4338CA",
      "--color-secondary": "#F472B6",
      "--color-secondary-hover": "#EC4899",
      "--color-success": "#10B981",
      "--color-warning": "#F59E0B",
      "--color-error": "#EF4444",
      "--color-text": "#0F172A",
      "--color-text-muted": "rgba(15, 23, 42, 0.70)",
      "--color-text-subtle": "rgba(15, 23, 42, 0.50)",
      "--color-glass": "rgba(255, 255, 255, 0.72)",
      "--color-glass-border": "rgba(15, 23, 42, 0.10)",
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(99, 102, 241, 0.13)",
      "--tcp-glow-b": "rgba(244, 114, 182, 0.11)",
    },
  },
  {
    id: "neon_riot",
    name: "Neon Riot",
    description: "Electric cyan and magenta over deep space surfaces.",
    cssVars: {
      "--color-background": "#050014",
      "--color-surface": "#0B0720",
      "--color-surface-elevated": "#140A3A",
      "--color-border": "rgba(248, 250, 252, 0.16)",
      "--color-border-subtle": "rgba(248, 250, 252, 0.10)",
      "--color-primary": "#00E5FF",
      "--color-primary-hover": "#00B8D4",
      "--color-primary-muted": "#00838F",
      "--color-secondary": "#FF3DF5",
      "--color-secondary-hover": "#D500F9",
      "--color-success": "#22C55E",
      "--color-warning": "#FBBF24",
      "--color-error": "#FB7185",
      "--color-text": "#F8FAFC",
      "--color-text-muted": "rgba(248, 250, 252, 0.72)",
      "--color-text-subtle": "rgba(248, 250, 252, 0.52)",
      "--color-glass": "rgba(5, 0, 20, 0.55)",
      "--color-glass-border": "rgba(255, 255, 255, 0.14)",
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(0, 229, 255, 0.20)",
      "--tcp-glow-b": "rgba(255, 61, 245, 0.14)",
    },
  },
  {
    id: "cosmic_glass",
    name: "Cosmic Glass",
    description: "Transparent nebula glass panels with deep-space glow.",
    cssVars: {
      "--color-background": "#03020F",
      "--color-surface": "rgba(9, 6, 28, 0.72)",
      "--color-surface-elevated": "rgba(18, 12, 40, 0.82)",
      "--color-border": "rgba(120, 105, 255, 0.22)",
      "--color-border-subtle": "rgba(120, 105, 255, 0.12)",
      "--color-primary": "#7C5CFF",
      "--color-primary-hover": "#6A40FF",
      "--color-primary-muted": "#4C2ED8",
      "--color-secondary": "#FF7AD9",
      "--color-secondary-hover": "#FF4FC3",
      "--color-success": "#3BE4C0",
      "--color-warning": "#F9D86B",
      "--color-error": "#FF5C7A",
      "--color-text": "#F3F0FF",
      "--color-text-muted": "rgba(243, 240, 255, 0.74)",
      "--color-text-subtle": "rgba(243, 240, 255, 0.52)",
      "--color-glass": "rgba(14, 10, 40, 0.48)",
      "--color-glass-border": "rgba(255, 255, 255, 0.16)",
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(124, 92, 255, 0.20)",
      "--tcp-glow-b": "rgba(255, 122, 217, 0.14)",
    },
  },
  {
    id: "pink_pony",
    name: "Pink Pony",
    description: "Candy pinks and dreamy pastels with brighter glass accents.",
    cssVars: {
      "--color-background": "#240818",
      "--color-surface": "rgba(56, 12, 38, 0.94)",
      "--color-surface-elevated": "rgba(72, 16, 48, 0.96)",
      "--color-border": "rgba(255, 184, 217, 0.36)",
      "--color-border-subtle": "rgba(255, 184, 217, 0.22)",
      "--color-primary": "#FF5FB1",
      "--color-primary-hover": "#FF3B9A",
      "--color-primary-muted": "#D91E7D",
      "--color-secondary": "#FFB8E6",
      "--color-secondary-hover": "#FF98D8",
      "--color-success": "#58E5C1",
      "--color-warning": "#FFD166",
      "--color-error": "#FF5C8A",
      "--color-text": "#FFF7FC",
      "--color-text-muted": "rgba(255, 247, 252, 0.86)",
      "--color-text-subtle": "rgba(255, 247, 252, 0.68)",
      "--color-glass": "rgba(74, 17, 50, 0.78)",
      "--color-glass-border": "rgba(255, 220, 238, 0.30)",
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(255, 95, 177, 0.20)",
      "--tcp-glow-b": "rgba(255, 184, 230, 0.14)",
    },
  },
  {
    id: "zen_dusk",
    name: "Zen Dusk",
    description: "Muted green dusk tones for calmer long-session work.",
    cssVars: {
      "--color-background": "#0B1110",
      "--color-surface": "#101716",
      "--color-surface-elevated": "#141D1C",
      "--color-border": "rgba(226, 232, 240, 0.12)",
      "--color-border-subtle": "rgba(226, 232, 240, 0.06)",
      "--color-primary": "#7CC8A4",
      "--color-primary-hover": "#6AB690",
      "--color-primary-muted": "#559B7B",
      "--color-secondary": "#9FB8B0",
      "--color-secondary-hover": "#8AA59C",
      "--color-success": "#5EC79B",
      "--color-warning": "#E6C17B",
      "--color-error": "#E38B8B",
      "--color-text": "#E6EFEA",
      "--color-text-muted": "rgba(230, 239, 234, 0.72)",
      "--color-text-subtle": "rgba(230, 239, 234, 0.50)",
      "--color-glass": "rgba(20, 28, 26, 0.72)",
      "--color-glass-border": "rgba(255, 255, 255, 0.08)",
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(124, 200, 164, 0.16)",
      "--tcp-glow-b": "rgba(159, 184, 176, 0.12)",
    },
  },
];

export const MOTION_TOKENS = {
  duration: {
    fast: 0.14,
    normal: 0.22,
    slow: 0.38,
  },
  easing: {
    standard: [0.22, 1, 0.36, 1] as const,
    soft: [0.33, 1, 0.68, 1] as const,
    exit: [0.4, 0, 1, 1] as const,
  },
  spring: {
    gentle: { type: "spring" as const, stiffness: 220, damping: 26, mass: 0.9 },
    snappy: { type: "spring" as const, stiffness: 320, damping: 30, mass: 0.82 },
    modal: { type: "spring" as const, stiffness: 280, damping: 28, mass: 0.95 },
    drawer: { type: "spring" as const, stiffness: 340, damping: 34, mass: 0.82 },
  },
};

export function getThemeById(themeId: string | null | undefined): ThemeDefinition {
  let id = String(themeId || "").trim() || DEFAULT_THEME_ID;
  if (id === "web_control") id = DEFAULT_THEME_ID;
  return THEMES.find((theme) => theme.id === id) || THEMES[0];
}

export function cycleThemeId(currentThemeId: string | null | undefined): ThemeId {
  const current = getThemeById(currentThemeId).id;
  const idx = THEMES.findIndex((theme) => theme.id === current);
  if (idx < 0) return DEFAULT_THEME_ID;
  return THEMES[(idx + 1) % THEMES.length]!.id;
}

export function applyThemeToDocument(
  themeId: string | null | undefined,
  root: HTMLElement | null | undefined = typeof document !== "undefined"
    ? document.documentElement
    : null
) {
  const theme = getThemeById(themeId);
  if (!root) return theme;
  for (const [name, value] of Object.entries(STRUCTURAL_THEME_VARS)) {
    root.style.setProperty(name, value);
  }
  for (const [name, value] of Object.entries(theme.cssVars)) {
    if (value == null) continue;
    root.style.setProperty(name, value);
  }
  root.dataset.theme = theme.id;
  root.style.colorScheme = theme.id === "porcelain" ? "light" : "dark";
  return theme;
}

export function prefersReducedMotion() {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return false;
  return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
}
