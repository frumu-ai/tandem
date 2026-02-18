import i18n from "./index";
import { getLanguageSetting, setLanguageSetting } from "@/lib/tauri";

type SupportedLanguage = "en" | "zh-CN";

const LANGUAGE_STORAGE_KEY = "tandem.language";

export function normalizeLanguage(code: string | null | undefined): SupportedLanguage {
  const normalized = (code ?? "").trim().toLowerCase();
  if (normalized.startsWith("zh")) return "zh-CN";
  return "en";
}

function readLocalLanguage(): SupportedLanguage | null {
  try {
    const value = localStorage.getItem(LANGUAGE_STORAGE_KEY);
    return value ? normalizeLanguage(value) : null;
  } catch {
    return null;
  }
}

function writeLocalLanguage(language: SupportedLanguage): void {
  try {
    localStorage.setItem(LANGUAGE_STORAGE_KEY, language);
  } catch {
    // ignore storage errors
  }
}

export async function persistLanguagePreference(language: string): Promise<void> {
  const normalized = normalizeLanguage(language);
  if (i18n.language !== normalized) {
    await i18n.changeLanguage(normalized);
  }
  writeLocalLanguage(normalized);
  try {
    await setLanguageSetting(normalized);
  } catch {
    // Backend persistence is best-effort so UI never blocks on store issues.
  }
}

export async function bootstrapLanguagePreference(): Promise<void> {
  const localLanguage = readLocalLanguage();
  const detectedLanguage = normalizeLanguage(i18n.language);

  let backendLanguage: SupportedLanguage | null = null;
  try {
    backendLanguage = normalizeLanguage(await getLanguageSetting());
  } catch {
    backendLanguage = null;
  }

  const resolved = backendLanguage ?? localLanguage ?? detectedLanguage;

  if (i18n.language !== resolved) {
    await i18n.changeLanguage(resolved);
  }
  writeLocalLanguage(resolved);

  // Keep backend in sync even when falling back to local/detected language.
  try {
    await setLanguageSetting(resolved);
  } catch {
    // ignore backend sync failures
  }
}
