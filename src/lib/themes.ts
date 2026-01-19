import type { ThemeDefinition, ThemeId } from "@/types/theme";

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

      // Primary accents (Solar Yellow)
      "--color-primary": "#F59E0B",
      "--color-primary-hover": "#D97706",
      "--color-primary-muted": "#B45309",

      // Secondary accents (Crimson Red)
      "--color-secondary": "#EF4444",
      "--color-secondary-hover": "#DC2626",

      "--color-success": "#10B981",
      // Keep warning/error semantic, but align with palette
      "--color-warning": "#F59E0B",
      "--color-error": "#EF4444",

      "--color-text": "#F5F5F5",
      "--color-text-muted": "rgba(245, 245, 245, 0.70)",
      "--color-text-subtle": "rgba(245, 245, 245, 0.50)",

      // Glassmorphism
      "--color-glass": "rgba(255, 255, 255, 0.03)",
      "--color-glass-border": "rgba(255, 255, 255, 0.08)",

      // Typography (installed in typography step)
      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
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
      "--font-mono":
        '"JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
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
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
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

      // Primary accents (Cherry Pink)
      "--color-primary": "#FB7185",
      "--color-primary-hover": "#F43F5E",
      "--color-primary-muted": "#E11D48",

      // Secondary accents (Lavender)
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
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
    },
  },
  {
    id: "porcelain",
    name: "Porcelain",
    description: "Plain, bright whites with soft pastel accents (easy on the eyes).",
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
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
    },
  },
  {
    id: "neon_riot",
    name: "Neon Riot",
    description: "Crazy cyber-neon: electric cyan + hot magenta on deep space black.",
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
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
    },
  },
];

export function getThemeById(id: ThemeId): ThemeDefinition {
  const theme = THEMES.find((t) => t.id === id);
  return theme ?? THEMES[0]!;
}

export function cycleThemeId(current: ThemeId): ThemeId {
  const idx = THEMES.findIndex((t) => t.id === current);
  if (idx === -1) return DEFAULT_THEME_ID;
  const next = (idx + 1) % THEMES.length;
  return THEMES[next]!.id;
}
