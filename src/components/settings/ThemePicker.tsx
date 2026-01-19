import { Check } from "lucide-react";
import { cn } from "@/lib/utils";
import { useTheme } from "@/hooks/useTheme";
import type { ThemeDefinition } from "@/types/theme";

function ThemeSwatches({ theme }: { theme: ThemeDefinition }) {
  const background = theme.cssVars["--color-background"] ?? "#000000";
  const surface = theme.cssVars["--color-surface"] ?? "#111111";
  const primary = theme.cssVars["--color-primary"] ?? "#ffffff";
  const secondary = theme.cssVars["--color-secondary"] ?? "#ffffff";

  return (
    <div className="flex items-center gap-2">
      <span
        className="h-4 w-4 rounded-full ring-1 ring-border"
        style={{ background }}
        aria-hidden="true"
      />
      <span
        className="h-4 w-4 rounded-full ring-1 ring-border"
        style={{ background: surface }}
        aria-hidden="true"
      />
      <span
        className="h-4 w-4 rounded-full ring-1 ring-border"
        style={{ background: primary }}
        aria-hidden="true"
      />
      <span
        className="h-4 w-4 rounded-full ring-1 ring-border"
        style={{ background: secondary }}
        aria-hidden="true"
      />
    </div>
  );
}

export function ThemePicker() {
  const { themeId, availableThemes, setThemeId } = useTheme();

  return (
    <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
      {availableThemes.map((theme) => {
        const selected = theme.id === themeId;

        return (
          <button
            key={theme.id}
            type="button"
            onClick={() => setThemeId(theme.id)}
            aria-pressed={selected}
            className={cn(
              "group relative rounded-xl border p-4 pb-10 text-left transition-all",
              "bg-surface hover:bg-surface-elevated",
              "border-border hover:border-border-subtle",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary",
              selected && "ring-2 ring-primary border-primary/40"
            )}
          >
            <div className="flex items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <p className="font-semibold text-text">{theme.name}</p>
                </div>
                <p className="mt-1 text-sm text-text-muted">{theme.description}</p>
              </div>
              <ThemeSwatches theme={theme} />
            </div>

            {/* Active badge (bottom-right, no layout shift) */}
            <span
              className={cn(
                "pointer-events-none absolute bottom-3 right-3 inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium",
                "bg-primary/20 text-primary transition-opacity",
                selected ? "opacity-100" : "opacity-0"
              )}
              aria-hidden={!selected}
            >
              <Check className="h-3 w-3" />
              Active
            </span>
          </button>
        );
      })}
    </div>
  );
}
