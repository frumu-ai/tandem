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
  {
    id: "cosmic_glass",
    name: "Cosmic Glass",
    description: "Funky deep-space glow with transparent nebula glass panels.",
    cssVars: {
      "--color-background":
        "radial-gradient(circle at 8% 14%, rgba(255, 255, 255, 0.75) 0 1px, transparent 2px), radial-gradient(circle at 16% 44%, rgba(255, 255, 255, 0.45) 0 1px, transparent 2px), radial-gradient(circle at 22% 72%, rgba(255, 255, 255, 0.35) 0 1px, transparent 2px), radial-gradient(circle at 28% 18%, rgba(255, 255, 255, 0.55) 0 1px, transparent 2px), radial-gradient(circle at 34% 56%, rgba(255, 255, 255, 0.30) 0 1px, transparent 2px), radial-gradient(circle at 42% 28%, rgba(255, 255, 255, 0.40) 0 1px, transparent 2px), radial-gradient(circle at 48% 78%, rgba(255, 255, 255, 0.28) 0 1px, transparent 2px), radial-gradient(circle at 56% 16%, rgba(255, 255, 255, 0.55) 0 1px, transparent 2px), radial-gradient(circle at 62% 46%, rgba(255, 255, 255, 0.34) 0 1px, transparent 2px), radial-gradient(circle at 70% 24%, rgba(255, 255, 255, 0.62) 0 1px, transparent 2px), radial-gradient(circle at 78% 58%, rgba(255, 255, 255, 0.32) 0 1px, transparent 2px), radial-gradient(circle at 86% 22%, rgba(255, 255, 255, 0.42) 0 1px, transparent 2px), radial-gradient(circle at 90% 74%, rgba(255, 255, 255, 0.28) 0 1px, transparent 2px), radial-gradient(circle at 12% 86%, rgba(255, 255, 255, 0.22) 0 1px, transparent 2px), radial-gradient(circle at 92% 10%, rgba(255, 255, 255, 0.22) 0 1px, transparent 2px), radial-gradient(circle at 65% 26%, rgba(255, 255, 255, 0.14) 0, transparent 36%), radial-gradient(ellipse 80% 56% at 65% 26%, rgba(124, 92, 255, 0.24) 0, transparent 58%), radial-gradient(ellipse 64% 44% at 58% 32%, rgba(255, 122, 217, 0.16) 0, transparent 62%), radial-gradient(ellipse 66% 46% at 74% 22%, rgba(59, 228, 192, 0.10) 0, transparent 62%), #03020F",
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
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
    },
  },
  {
    id: "pink_pony",
    name: "Pink Pony",
    description: "Super girly Barbie-core glow with candy pinks and dreamy pastels.",
    cssVars: {
      "--color-background":
        "radial-gradient(120% 90% at 50% 125%, transparent 0 48%, rgba(255, 255, 255, 0.12) 48% 49%, rgba(255, 82, 136, 0.86) 49% 52%, rgba(255, 160, 104, 0.82) 52% 55%, rgba(255, 235, 140, 0.78) 55% 58%, rgba(140, 255, 208, 0.74) 58% 61%, rgba(122, 221, 255, 0.74) 61% 64%, rgba(174, 171, 255, 0.74) 64% 67%, rgba(238, 182, 255, 0.70) 67% 70%, transparent 70% 100%), radial-gradient(circle at 18% 22%, rgba(255, 255, 255, 0.10) 0, transparent 55%), radial-gradient(circle at 80% 18%, rgba(255, 255, 255, 0.08) 0, transparent 58%), linear-gradient(135deg, #FF5FA2 0%, #FF8AD6 28%, #FFD166 52%, #9BF6FF 76%, #BDB2FF 100%)",
      "--color-surface": "rgba(64, 16, 42, 0.82)",
      "--color-surface-elevated": "rgba(86, 20, 56, 0.88)",
      "--color-border": "rgba(255, 158, 204, 0.28)",
      "--color-border-subtle": "rgba(255, 158, 204, 0.14)",

      "--color-primary": "#FF5FB1",
      "--color-primary-hover": "#FF3B9A",
      "--color-primary-muted": "#D91E7D",

      "--color-secondary": "#FFB3E6",
      "--color-secondary-hover": "#FF8DD6",

      "--color-success": "#58E5C1",
      "--color-warning": "#FFD166",
      "--color-error": "#FF5C8A",

      "--color-text": "#FFF1FA",
      "--color-text-muted": "rgba(255, 241, 250, 0.74)",
      "--color-text-subtle": "rgba(255, 241, 250, 0.52)",

      "--color-glass": "rgba(255, 105, 180, 0.18)",
      "--color-glass-border": "rgba(255, 255, 255, 0.22)",

      "--font-sans": '"Geist Sans", "Inter", system-ui, -apple-system, sans-serif',
      "--font-mono":
        '"Geist Mono", "JetBrains Mono", "Fira Code", ui-monospace, SFMono-Regular, Menlo, monospace',
    },
  },
  {
    id: "zen_dusk",
    name: "Zen Dusk",
    description: "Low-contrast calm dark mode with muted sage and soft ink tones.",
    cssVars: {
      "--color-background":
        "radial-gradient(circle at 18% 12%, rgba(124, 200, 164, 0.10) 0, transparent 42%), radial-gradient(circle at 78% 32%, rgba(159, 184, 176, 0.07) 0, transparent 46%), radial-gradient(140% 110% at 50% 120%, rgba(0, 0, 0, 0.40) 0, transparent 58%), linear-gradient(145deg, #0B1110 0%, #0C1412 52%, #08100F 100%)",
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
