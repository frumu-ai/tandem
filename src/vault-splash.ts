// Vault PIN entry logic for splash screen
// This runs before the main React app loads

import { invoke } from "@tauri-apps/api/core";
import { DEFAULT_THEME_ID, getThemeById } from "./lib/themes";
import type { ThemeId } from "./types/theme";

const MIN_PIN_LENGTH = 4;
const MAX_PIN_LENGTH = 4;

let currentPin = "";
let confirmPin = "";
let isCreateMode = false;
let isConfirmStep = false;
let isLoading = false;

type VaultStatus = "not_created" | "locked" | "unlocked";

function parseRgb(color: string): { r: number; g: number; b: number } | null {
  const c = color.trim();
  if (!c) return null;

  // rgb()/rgba()
  const rgbMatch = c.match(
    /^rgba?\(\s*(\d{1,3})\s*,\s*(\d{1,3})\s*,\s*(\d{1,3})(?:\s*,\s*[\d.]+\s*)?\)$/
  );
  if (rgbMatch) {
    return { r: Number(rgbMatch[1]), g: Number(rgbMatch[2]), b: Number(rgbMatch[3]) };
  }

  // #rgb / #rrggbb
  const hex = c.startsWith("#") ? c.slice(1) : c;
  if (hex.length === 3) {
    const r = Number.parseInt(hex[0] + hex[0], 16);
    const g = Number.parseInt(hex[1] + hex[1], 16);
    const b = Number.parseInt(hex[2] + hex[2], 16);
    return { r, g, b };
  }
  if (hex.length === 6) {
    const r = Number.parseInt(hex.slice(0, 2), 16);
    const g = Number.parseInt(hex.slice(2, 4), 16);
    const b = Number.parseInt(hex.slice(4, 6), 16);
    return { r, g, b };
  }

  return null;
}

function applySplashTheme() {
  const themeId = (localStorage.getItem("tandem.themeId") as ThemeId | null) ?? DEFAULT_THEME_ID;
  const theme = getThemeById(themeId);

  // Apply core theme vars early (so splash + first paint match)
  for (const [name, value] of Object.entries(theme.cssVars)) {
    if (value == null) continue;
    document.documentElement.style.setProperty(name, value);
  }
  document.documentElement.dataset.theme = theme.id;

  const splash = document.getElementById("splash-screen");
  if (!splash) return;

  const accent = theme.cssVars["--color-primary"] ?? "#00ff88";
  const bg = theme.cssVars["--color-background"] ?? "#000000";
  const text = theme.cssVars["--color-text"] ?? "#ffffff";
  const err = theme.cssVars["--color-error"] ?? "#ff4444";

  splash.style.setProperty("--matrix-green", accent);
  splash.style.setProperty("--matrix-bg", bg);
  splash.style.setProperty("--matrix-text", text);
  splash.style.setProperty("--error-red", err);

  const accentRgb = parseRgb(accent);
  const bgRgb = parseRgb(bg);
  const textRgb = parseRgb(text);
  if (accentRgb) {
    splash.style.setProperty(
      "--matrix-green-rgb",
      `${accentRgb.r}, ${accentRgb.g}, ${accentRgb.b}`
    );
  }
  if (bgRgb) {
    splash.style.setProperty("--matrix-bg-rgb", `${bgRgb.r}, ${bgRgb.g}, ${bgRgb.b}`);
  }
  if (textRgb) {
    splash.style.setProperty("--matrix-text-rgb", `${textRgb.r}, ${textRgb.g}, ${textRgb.b}`);
  }

  // Reveal the splash screen now that theme is applied
  splash.style.opacity = "1";
}

function startMatrixRain() {
  const canvasEl = document.getElementById("matrix-canvas");
  const splashEl = document.getElementById("splash-screen");
  if (!(canvasEl instanceof HTMLCanvasElement) || !(splashEl instanceof HTMLElement)) return;

  const canvas = canvasEl;
  const splash = splashEl;

  const ctx = canvas.getContext("2d");
  if (!ctx) return;
  const context = ctx;

  const chars =
    "アイウエオカキクケコサシスセソタチツテトナニヌネノハヒフヘホマミムメモヤユヨラリルレロワヲン0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ";
  const fontSize = 14;

  function readVar(name: string, fallback: string) {
    const v = getComputedStyle(splash).getPropertyValue(name).trim();
    return v || fallback;
  }

  function resize() {
    canvas.width = window.innerWidth;
    canvas.height = window.innerHeight;
    columns = Math.floor(canvas.width / fontSize);
    drops = Array(columns).fill(1);
  }

  let columns = 0;
  let drops: number[] = [];
  resize();
  window.addEventListener("resize", resize);

  function draw() {
    const bgRgb = readVar("--matrix-bg-rgb", "0, 0, 0");
    const accent = readVar("--matrix-green", "#00ff88");
    const text = readVar("--matrix-text", "#ffffff");

    context.fillStyle = `rgba(${bgRgb}, 0.08)`;
    context.fillRect(0, 0, canvas.width, canvas.height);

    context.font = `${fontSize}px monospace`;

    for (let i = 0; i < drops.length; i++) {
      const char = chars[Math.floor(Math.random() * chars.length)];
      const x = i * fontSize;
      const y = drops[i] * fontSize;

      // Brighter head for the top area
      context.fillStyle = y < 50 ? text : accent;
      context.globalAlpha = Math.random() * 0.5 + 0.5;
      context.fillText(char, x, y);
      context.globalAlpha = 1;

      if (y > canvas.height && Math.random() > 0.975) {
        drops[i] = 0;
      }
      drops[i]++;
    }
  }

  const interval = window.setInterval(draw, 33);
  (window as any).__matrixInterval = interval;
}

// Get DOM elements
const loadingSection = document.getElementById("loading-section")!;
const pinSection = document.getElementById("pin-section")!;
const pinTitle = document.getElementById("pin-title")!;
const pinSubtitle = document.getElementById("pin-subtitle")!;
const pinDots = document.getElementById("pin-dots")!;
const pinError = document.getElementById("pin-error")!;
const pinConfirmHint = document.getElementById("pin-confirm-hint")!;
const loadingText = document.getElementById("loading-text")!;

// Theme + matrix start (must happen before we begin interactions)
applySplashTheme();
startMatrixRain();

function updatePinDots() {
  const dots = pinDots.querySelectorAll(".pin-dot");
  dots.forEach((dot, i) => {
    dot.classList.toggle("filled", i < currentPin.length);
  });
}

function showError(message: string) {
  pinError.textContent = message;
  const dots = pinDots.querySelectorAll(".pin-dot");
  dots.forEach((dot) => dot.classList.add("error"));
  setTimeout(() => {
    dots.forEach((dot) => dot.classList.remove("error"));
  }, 300);
}

function clearError() {
  pinError.textContent = "";
}

function setLoading(loading: boolean) {
  isLoading = loading;
  pinSection.classList.toggle("loading", loading);
}

function showPinUI(createMode: boolean) {
  isCreateMode = createMode;
  isConfirmStep = false;
  currentPin = "";
  confirmPin = "";

  loadingSection.style.display = "none";
  pinSection.classList.add("visible");

  if (createMode) {
    pinTitle.textContent = "Create Your PIN";
    pinSubtitle.textContent = "Secure your vault with a 4 digit PIN";
    pinConfirmHint.style.display = "none";
  } else {
    pinTitle.textContent = "Enter Your PIN";
    pinSubtitle.textContent = "Unlock your secure vault";
    pinConfirmHint.style.display = "none";
  }

  updatePinDots();
  clearError();
}

function showConfirmStep() {
  isConfirmStep = true;
  confirmPin = currentPin;
  currentPin = "";

  pinTitle.textContent = "Confirm Your PIN";
  pinSubtitle.textContent = "Enter the same PIN again";
  pinConfirmHint.textContent = "Re-enter your PIN to confirm";
  pinConfirmHint.style.display = "block";

  updatePinDots();
  clearError();
}

async function submitPin() {
  if (currentPin.length < MIN_PIN_LENGTH) {
    showError("PIN must be at least " + MIN_PIN_LENGTH + " digits");
    return;
  }

  setLoading(true);

  try {
    if (isCreateMode) {
      if (!isConfirmStep) {
        // First entry - show confirm step
        showConfirmStep();
        setLoading(false);
        return;
      }

      // Confirm step - check if PINs match
      if (currentPin !== confirmPin) {
        showError("PINs do not match");
        currentPin = "";
        updatePinDots();
        isConfirmStep = false;
        showPinUI(true);
        setLoading(false);
        return;
      }

      // Create vault
      loadingText.innerHTML = 'Creating secure vault<span class="loading-dots"></span>';
      loadingSection.style.display = "flex";
      pinSection.classList.remove("visible");

      await invoke("create_vault", { pin: currentPin });
      (window as any).__vaultUnlocked = true;
    } else {
      // Unlock existing vault
      loadingText.innerHTML = 'Unlocking vault<span class="loading-dots"></span>';
      loadingSection.style.display = "flex";
      pinSection.classList.remove("visible");

      await invoke("unlock_vault", { pin: currentPin });
      (window as any).__vaultUnlocked = true;
    }

    // Success! The React app will handle the rest
    console.log("[Vault] Unlocked successfully");
  } catch (error: any) {
    console.error("[Vault] Error:", error);
    setLoading(false);
    loadingSection.style.display = "none";
    pinSection.classList.add("visible");

    if (error.toString().includes("Invalid PIN")) {
      showError("Incorrect PIN");
    } else {
      showError("Error: " + (error.message || error));
    }

    currentPin = "";
    updatePinDots();
  }
}

function handleKeyPress(key: string) {
  if (isLoading) return;

  clearError();

  if (key === "delete") {
    currentPin = currentPin.slice(0, -1);
  } else if (key === "clear") {
    currentPin = "";
  } else if (key >= "0" && key <= "9") {
    if (currentPin.length < MAX_PIN_LENGTH) {
      currentPin += key;
    }
  }

  updatePinDots();

  // Auto-submit when max length reached
  if (currentPin.length === MAX_PIN_LENGTH) {
    submitPin();
  }
}

// Keypad click handlers
document.querySelectorAll(".pin-key").forEach((button) => {
  button.addEventListener("click", () => {
    const key = (button as HTMLElement).dataset.key;
    if (key) handleKeyPress(key);
  });
});

// Keyboard support
document.addEventListener("keydown", (e) => {
  if (!pinSection.classList.contains("visible")) return;

  if (e.key >= "0" && e.key <= "9") {
    handleKeyPress(e.key);
  } else if (e.key === "Backspace") {
    handleKeyPress("delete");
  } else if (e.key === "Escape") {
    handleKeyPress("clear");
  } else if (e.key === "Enter" && currentPin.length >= MIN_PIN_LENGTH) {
    submitPin();
  }
});

// Check vault status
async function checkVaultStatus() {
  try {
    console.log("[Vault] Checking status...");
    loadingText.innerHTML = 'Checking vault<span class="loading-dots"></span>';

    const status = (await invoke("get_vault_status")) as VaultStatus;
    console.log("[Vault] Status:", status);

    if (status === "not_created") {
      showPinUI(true);
    } else if (status === "locked") {
      showPinUI(false);
    } else if (status === "unlocked") {
      // Already unlocked (shouldn't happen normally)
      (window as any).__vaultUnlocked = true;
    }
  } catch (error: any) {
    console.error("[Vault] Failed to check status:", error);
    // Show error state but allow retry
    loadingText.innerHTML =
      "Error: " + (error.message || error) + '<span class="loading-dots"></span>';
    setTimeout(checkVaultStatus, 2000);
  }
}

// Start checking vault status
checkVaultStatus();
