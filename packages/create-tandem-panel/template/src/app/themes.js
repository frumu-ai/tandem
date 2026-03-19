import {
  DEFAULT_THEME_ID,
  THEMES,
  applyThemeToDocument,
  cycleThemeId,
  getThemeById,
  MOTION_TOKENS,
  prefersReducedMotion,
} from "./themeContract.ts";

const STORAGE_KEY = "tandem.themeId";

export { DEFAULT_THEME_ID, THEMES, cycleThemeId, getThemeById, MOTION_TOKENS, prefersReducedMotion };

export function getActiveThemeId() {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    return getThemeById(saved).id;
  } catch {
    return DEFAULT_THEME_ID;
  }
}

export function applyTheme(themeId) {
  return applyThemeToDocument(themeId);
}

export function setControlPanelTheme(themeId) {
  const theme = applyThemeToDocument(themeId);
  try {
    localStorage.setItem(STORAGE_KEY, theme.id);
  } catch {
    // ignore storage failures
  }
  return theme;
}
