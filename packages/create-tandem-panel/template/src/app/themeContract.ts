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

export const DEFAULT_THEME_ID: ThemeId = "charcoal_fire";

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
      "--color-primary-muted": "#5132F0",
      "--color-secondary": "#29D3FF",
      "--color-secondary-hover": "#00B8F0",
      "--color-success": "#34D399",
      "--color-warning": "#FBBF24",
      "--color-error": "#FB7185",
      "--color-text": "#F8FAFC",
      "--color-text-muted": "rgba(248, 250, 252, 0.72)",
      "--color-text-subtle": "rgba(248, 250, 252, 0.52)",
      "--color-glass": "rgba(255, 255, 255, 0.04)",
      "--color-glass-border": "rgba(255, 255, 255, 0.10)",
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(124, 92, 255, 0.20)",
      "--tcp-glow-b": "rgba(41, 211, 255, 0.14)",
    },
  },
  {
    id: "pink_pony",
    name: "Pink Pony",
    description: "Bright candy pinks with glossy glass panels and playful energy.",
    cssVars: {
      "--color-background": "#1B0A16",
      "--color-surface": "#241021",
      "--color-surface-elevated": "#2D1529",
      "--color-border": "rgba(255, 215, 232, 0.14)",
      "--color-border-subtle": "rgba(255, 215, 232, 0.08)",
      "--color-primary": "#FF5FA2",
      "--color-primary-hover": "#FF3C8C",
      "--color-primary-muted": "#E11D70",
      "--color-secondary": "#FFD166",
      "--color-secondary-hover": "#FBBF24",
      "--color-success": "#34D399",
      "--color-warning": "#F59E0B",
      "--color-error": "#FB7185",
      "--color-text": "#FFF1F7",
      "--color-text-muted": "rgba(255, 241, 247, 0.74)",
      "--color-text-subtle": "rgba(255, 241, 247, 0.52)",
      "--color-glass": "rgba(255, 255, 255, 0.04)",
      "--color-glass-border": "rgba(255, 255, 255, 0.10)",
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(255, 95, 162, 0.18)",
      "--tcp-glow-b": "rgba(255, 209, 102, 0.12)",
    },
  },
  {
    id: "zen_dusk",
    name: "Zen Dusk",
    description: "Muted twilight surfaces with calm blue-green accents.",
    cssVars: {
      "--color-background": "#0F1720",
      "--color-surface": "#16202A",
      "--color-surface-elevated": "#1D2A36",
      "--color-border": "rgba(226, 232, 240, 0.12)",
      "--color-border-subtle": "rgba(226, 232, 240, 0.08)",
      "--color-primary": "#38BDF8",
      "--color-primary-hover": "#0EA5E9",
      "--color-primary-muted": "#0284C7",
      "--color-secondary": "#4ADE80",
      "--color-secondary-hover": "#22C55E",
      "--color-success": "#22C55E",
      "--color-warning": "#FBBF24",
      "--color-error": "#F87171",
      "--color-text": "#E2E8F0",
      "--color-text-muted": "rgba(226, 232, 240, 0.72)",
      "--color-text-subtle": "rgba(226, 232, 240, 0.52)",
      "--color-glass": "rgba(255, 255, 255, 0.04)",
      "--color-glass-border": "rgba(255, 255, 255, 0.10)",
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-display": '"Rubik", "Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
      "--tcp-glow-a": "rgba(56, 189, 248, 0.16)",
      "--tcp-glow-b": "rgba(74, 222, 128, 0.12)",
    },
  },
];

export function getThemeById(themeId: string | null | undefined) {
  return THEMES.find((theme) => theme.id === themeId) || THEMES[0];
}

export function applyThemeToDocument(themeId: string | null | undefined) {
  const theme = getThemeById(themeId);
  const root = document.documentElement;
  for (const [key, value] of Object.entries(theme.cssVars)) {
    root.style.setProperty(key, value);
  }
  root.dataset.theme = theme.id;
  return theme;
}

export function cycleThemeId(currentThemeId: string | null | undefined) {
  const currentIndex = THEMES.findIndex((theme) => theme.id === currentThemeId);
  const nextIndex = currentIndex >= 0 ? (currentIndex + 1) % THEMES.length : 0;
  return THEMES[nextIndex]?.id || DEFAULT_THEME_ID;
}

export const MOTION_TOKENS = {
  fast: 0.18,
  normal: 0.28,
  slow: 0.44,
};

export function prefersReducedMotion() {
  try {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  } catch {
    return false;
  }
}
