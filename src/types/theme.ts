export type ThemeId =
  | "charcoal_fire"
  | "electric_blue"
  | "emerald_night"
  | "hello_bunny"
  | "porcelain"
  | "neon_riot";

export type CssVarName =
  | "--color-background"
  | "--color-surface"
  | "--color-surface-elevated"
  | "--color-border"
  | "--color-border-subtle"
  | "--color-primary"
  | "--color-primary-hover"
  | "--color-primary-muted"
  | "--color-secondary"
  | "--color-secondary-hover"
  | "--color-success"
  | "--color-warning"
  | "--color-error"
  | "--color-text"
  | "--color-text-muted"
  | "--color-text-subtle"
  | "--color-glass"
  | "--color-glass-border"
  | "--font-sans"
  | "--font-mono";

export interface ThemeDefinition {
  id: ThemeId;
  name: string;
  description: string;
  cssVars: Partial<Record<CssVarName, string>>;
}
