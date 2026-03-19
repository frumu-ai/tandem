import { AnimatePresence, motion } from "motion/react";
import { MOTION_TOKENS } from "../app/themes.js";

function ThemeSwatches({ theme }: { theme: any }) {
  const background = theme?.cssVars?.["--color-background"] || "#000000";
  const surface = theme?.cssVars?.["--color-surface"] || "#111111";
  const primary = theme?.cssVars?.["--color-primary"] || "#ffffff";
  const secondary = theme?.cssVars?.["--color-secondary"] || "#ffffff";

  return (
    <div className="flex items-center gap-2">
      <span className="tcp-theme-swatch" style={{ background }} aria-hidden="true"></span>
      <span className="tcp-theme-swatch" style={{ background: surface }} aria-hidden="true"></span>
      <span className="tcp-theme-swatch" style={{ background: primary }} aria-hidden="true"></span>
      <span
        className="tcp-theme-swatch"
        style={{ background: secondary }}
        aria-hidden="true"
      ></span>
    </div>
  );
}

export function ThemePicker({
  themes,
  themeId,
  onChange,
}: {
  themes: any[];
  themeId: string;
  onChange: (themeId: string) => void;
}) {
  return (
    <div className="grid grid-cols-1 gap-3 md:grid-cols-2">
      {themes.map((theme) => {
        const selected = theme.id === themeId;
        return (
          <motion.button
            key={theme.id}
            type="button"
            layout
            onClick={() => onChange(theme.id)}
            className={`tcp-theme-tile ${selected ? "active" : ""}`}
            whileHover={{ y: -2 }}
            whileTap={{ scale: 0.985 }}
            transition={MOTION_TOKENS.spring.gentle}
          >
            <div className="tcp-theme-tile-top">
              <div className="min-w-0">
                <div className="tcp-theme-tile-title">{theme.name}</div>
                <p className="tcp-subtle mt-1 text-xs">{theme.description}</p>
              </div>
              <ThemeSwatches theme={theme} />
            </div>
            <div className="tcp-theme-preview">
              <div
                className="tcp-theme-preview-bg"
                style={{ background: theme.cssVars["--color-background"] || "#000" }}
              >
                <div
                  className="tcp-theme-preview-card"
                  style={{
                    background: theme.cssVars["--color-surface"] || "#111",
                    borderColor: theme.cssVars["--color-border"] || "rgba(255,255,255,0.1)",
                  }}
                >
                  <div
                    className="tcp-theme-preview-line short"
                    style={{ background: theme.cssVars["--color-text"] || "#fff" }}
                  ></div>
                  <div
                    className="tcp-theme-preview-line"
                    style={{ background: theme.cssVars["--color-text-muted"] || "#aaa" }}
                  ></div>
                  <div
                    className="tcp-theme-preview-pill"
                    style={{ background: theme.cssVars["--color-primary"] || "#fff" }}
                  ></div>
                </div>
              </div>
            </div>
            <AnimatePresence initial={false}>
              {selected ? (
                <motion.span
                  className="tcp-theme-active"
                  initial={{ opacity: 0, scale: 0.86 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.86 }}
                  transition={MOTION_TOKENS.spring.snappy}
                >
                  <i data-lucide="badge-check"></i>
                  Active
                </motion.span>
              ) : null}
            </AnimatePresence>
          </motion.button>
        );
      })}
    </div>
  );
}
