import { convertFileSrc } from "@tauri-apps/api/core";
import type { CustomBackgroundFit, CustomBackgroundInfo } from "@/lib/tauri";

export const CUSTOM_BG_STORAGE_KEY = "tandem.customBackground";
export const CUSTOM_BG_MAX_BYTES = 20 * 1024 * 1024;

export type CustomBackgroundMirror = {
  enabled: boolean;
  opacity: number; // 0..1
  fit: CustomBackgroundFit;
  filePath: string | null;
};

function normalizeNativePath(p: string): string {
  // Tauri and browsers typically tolerate Windows paths, but normalizing avoids edge cases
  // across platforms and protocol handlers.
  return p.replace(/\\/g, "/");
}

function fitToCss(fit: CustomBackgroundFit): {
  size: string;
  position: string;
  repeat: string;
} {
  switch (fit) {
    case "contain":
      return { size: "contain", position: "center", repeat: "no-repeat" };
    case "tile":
      return { size: "auto", position: "top left", repeat: "repeat" };
    case "cover":
    default:
      return { size: "cover", position: "center", repeat: "no-repeat" };
  }
}

export function mimeFromPath(path: string): string {
  const lower = path.toLowerCase();
  if (lower.endsWith(".png")) return "image/png";
  if (lower.endsWith(".jpg") || lower.endsWith(".jpeg")) return "image/jpeg";
  if (lower.endsWith(".webp")) return "image/webp";
  return "application/octet-stream";
}

export function getCustomBackgroundAssetUrl(
  info: CustomBackgroundInfo | null | undefined
): string | null {
  if (!info?.settings?.enabled || !info.file_path) return null;
  try {
    return convertFileSrc(normalizeNativePath(info.file_path));
  } catch {
    return null;
  }
}

export function applyCustomBackgroundUrl(
  settings: { opacity?: number; fit: CustomBackgroundFit },
  srcUrl: string
) {
  const root = document.documentElement;
  const css = fitToCss(settings.fit);

  root.style.setProperty("--custom-bg-image", `url("${srcUrl}")`);
  root.style.setProperty("--custom-bg-opacity", String(settings.opacity ?? 0));
  root.style.setProperty("--custom-bg-size", css.size);
  root.style.setProperty("--custom-bg-position", css.position);
  root.style.setProperty("--custom-bg-repeat", css.repeat);
}

export function setCustomBackgroundOpacity(opacity: number) {
  const root = document.documentElement;
  root.style.setProperty("--custom-bg-opacity", String(opacity ?? 0));
}

export function applyCustomBackground(info: CustomBackgroundInfo | null | undefined) {
  const root = document.documentElement;

  if (!info || !info.settings?.enabled || !info.file_path) {
    root.style.setProperty("--custom-bg-image", "none");
    root.style.setProperty("--custom-bg-opacity", "0");
    return;
  }

  const { opacity, fit } = info.settings;
  const src = getCustomBackgroundAssetUrl(info);
  if (!src) {
    // If convertFileSrc isn't available (e.g. web dev), skip.
    root.style.setProperty("--custom-bg-image", "none");
    root.style.setProperty("--custom-bg-opacity", "0");
    return;
  }

  applyCustomBackgroundUrl({ opacity, fit }, src);
}

export function mirrorCustomBackgroundToLocalStorage(
  info: CustomBackgroundInfo | null | undefined
) {
  const mirror: CustomBackgroundMirror = {
    enabled: !!info?.settings?.enabled && !!info?.file_path,
    opacity: info?.settings?.opacity ?? 0,
    fit: info?.settings?.fit ?? "cover",
    filePath: info?.file_path ?? null,
  };

  try {
    localStorage.setItem(CUSTOM_BG_STORAGE_KEY, JSON.stringify(mirror));
  } catch {
    // ignore storage failures
  }
}

export function readCustomBackgroundMirror(): CustomBackgroundMirror | null {
  try {
    const raw = localStorage.getItem(CUSTOM_BG_STORAGE_KEY);
    if (!raw) return null;
    return JSON.parse(raw) as CustomBackgroundMirror;
  } catch {
    return null;
  }
}

export function applyCustomBackgroundFromMirror(mirror: CustomBackgroundMirror | null) {
  if (!mirror?.enabled || !mirror.filePath) {
    applyCustomBackground(null);
    return;
  }

  applyCustomBackground({
    settings: {
      enabled: mirror.enabled,
      file_name: null,
      fit: mirror.fit,
      opacity: mirror.opacity,
    },
    file_path: mirror.filePath,
  });
}

export async function tryReadCustomBackgroundDataUrl(filePath: string): Promise<string | null> {
  // Only used as a fallback when asset protocol URLs fail to load (observed in some packaged builds).
  // We avoid mirroring this into localStorage because it can be very large.
  try {
    const { readBinaryFile } = await import("./tauri");
    const base64 = await readBinaryFile(filePath, CUSTOM_BG_MAX_BYTES);
    const mime = mimeFromPath(filePath);
    return `data:${mime};base64,${base64}`;
  } catch {
    return null;
  }
}
