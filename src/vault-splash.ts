// Vault PIN entry logic for splash screen
// This runs before the main React app loads

import { invoke } from "@tauri-apps/api/core";

const MIN_PIN_LENGTH = 4;
const MAX_PIN_LENGTH = 4;

let currentPin = "";
let confirmPin = "";
let isCreateMode = false;
let isConfirmStep = false;
let isLoading = false;

type VaultStatus = "not_created" | "locked" | "unlocked";

// Get DOM elements
const loadingSection = document.getElementById("loading-section")!;
const pinSection = document.getElementById("pin-section")!;
const pinTitle = document.getElementById("pin-title")!;
const pinSubtitle = document.getElementById("pin-subtitle")!;
const pinDots = document.getElementById("pin-dots")!;
const pinError = document.getElementById("pin-error")!;
const pinConfirmHint = document.getElementById("pin-confirm-hint")!;
const loadingText = document.getElementById("loading-text")!;

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
