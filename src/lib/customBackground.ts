import { convertFileSrc } from "@tauri-apps/api/core";
import type { CustomBackgroundFit, CustomBackgroundInfo } from "@/lib/tauri";

export const CUSTOM_BG_STORAGE_KEY = "tandem.customBackground";

export type CustomBackgroundMirror = {
  enabled: boolean;
  opacity: number; // 0..1
  fit: CustomBackgroundFit;
  filePath: string | null;
};

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

export function applyCustomBackground(info: CustomBackgroundInfo | null | undefined) {
  const root = document.documentElement;

  if (!info || !info.settings?.enabled || !info.file_path) {
    root.style.setProperty("--custom-bg-image", "none");
    root.style.setProperty("--custom-bg-opacity", "0");
    return;
  }

  const { opacity, fit } = info.settings;
  const css = fitToCss(fit);

  let src = "";
  try {
    src = convertFileSrc(info.file_path);
  } catch {
    // If convertFileSrc isn't available (e.g. web dev), skip.
    root.style.setProperty("--custom-bg-image", "none");
    root.style.setProperty("--custom-bg-opacity", "0");
    return;
  }

  root.style.setProperty("--custom-bg-image", `url("${src}")`);
  root.style.setProperty("--custom-bg-opacity", String(opacity ?? 0));
  root.style.setProperty("--custom-bg-size", css.size);
  root.style.setProperty("--custom-bg-position", css.position);
  root.style.setProperty("--custom-bg-repeat", css.repeat);
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
