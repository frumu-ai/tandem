import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Check, ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";
import { useTheme } from "@/hooks/useTheme";
import type { ThemeDefinition } from "@/types/theme";
import { useTranslation } from "react-i18next";

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

type ThemePickerVariant = "grid" | "compact";

export function ThemePicker({ variant = "grid" }: { variant?: ThemePickerVariant }) {
  const { t } = useTranslation("settings");
  const { themeId, availableThemes, setThemeId } = useTheme();
  const activeTheme = availableThemes.find((t) => t.id === themeId) ?? availableThemes[0]!;
  const [isOpen, setIsOpen] = useState(false);
  const themeName = (theme: ThemeDefinition) =>
    t(`theme.catalog.${theme.id}.name`, { defaultValue: theme.name });
  const themeDescription = (theme: ThemeDefinition) =>
    t(`theme.catalog.${theme.id}.description`, { defaultValue: theme.description });

  if (variant === "compact") {
    return (
      <div className="flex flex-col gap-2">
        <div className="flex items-center justify-between gap-3">
          <span className="text-sm font-medium text-text">
            {t("theme.selectTheme", { defaultValue: "Theme" })}
          </span>
          <ThemeSwatches theme={activeTheme} />
        </div>

        <div className="relative">
          <button
            type="button"
            onClick={() => setIsOpen((v) => !v)}
            aria-haspopup="listbox"
            aria-expanded={isOpen}
            className={cn(
              "flex w-full items-center justify-between gap-3 rounded-lg border px-3 py-2 text-left",
              "bg-surface hover:bg-surface-elevated",
              "border-border hover:border-border-subtle",
              "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary"
            )}
          >
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm font-medium text-text">{themeName(activeTheme)}</p>
              <p className="truncate text-xs text-text-muted">{themeDescription(activeTheme)}</p>
            </div>
            <ChevronDown
              className={cn(
                "h-4 w-4 flex-shrink-0 text-text-muted transition-transform",
                isOpen && "rotate-180"
              )}
            />
          </button>

          <AnimatePresence>
            {isOpen && (
              <>
                {/* Backdrop */}
                <div className="fixed inset-0 z-40" onClick={() => setIsOpen(false)} />

                {/* Menu */}
                <motion.div
                  initial={{ opacity: 0, y: -8 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -8 }}
                  transition={{ duration: 0.15 }}
                  role="listbox"
                  aria-label={t("theme.selectTheme", { defaultValue: "Select theme" })}
                  className="absolute left-0 right-0 top-full z-50 mt-2 max-h-80 overflow-y-auto rounded-lg border border-border bg-surface shadow-lg"
                >
                  {availableThemes.map((theme) => {
                    const selected = theme.id === themeId;
                    return (
                      <button
                        key={theme.id}
                        type="button"
                        role="option"
                        aria-selected={selected}
                        onClick={() => {
                          setIsOpen(false);
                          setThemeId(theme.id);
                        }}
                        className={cn(
                          "flex w-full items-start justify-between gap-3 px-3 py-2.5 text-left transition-colors",
                          "hover:bg-surface-elevated",
                          selected && "bg-primary/10"
                        )}
                      >
                        <div className="min-w-0 flex-1">
                          <p className="text-sm font-medium text-text">{themeName(theme)}</p>
                          <p className="text-xs text-text-muted">{themeDescription(theme)}</p>
                        </div>
                        <div className="mt-0.5 flex flex-shrink-0 items-center gap-2">
                          <ThemeSwatches theme={theme} />
                          {selected && <Check className="h-4 w-4 text-primary" />}
                        </div>
                      </button>
                    );
                  })}
                </motion.div>
              </>
            )}
          </AnimatePresence>
        </div>
      </div>
    );
  }

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
                  <p className="font-semibold text-text">{themeName(theme)}</p>
                </div>
                <p className="mt-1 text-sm text-text-muted">{themeDescription(theme)}</p>
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
              {t("theme.active", { defaultValue: "Active" })}
            </span>
          </button>
        );
      })}
    </div>
  );
}
