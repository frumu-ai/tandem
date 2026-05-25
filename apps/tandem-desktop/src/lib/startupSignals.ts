export const STARTUP_ERROR_EVENT = "tandem-startup-error";
export const DESKTOP_VISIBLE_EVENT = "tandem-desktop-visible";

function toStartupMessage(reason: unknown): string {
  if (reason instanceof Error) {
    return reason.message;
  }
  if (typeof reason === "string" && reason.trim().length > 0) {
    return reason.trim();
  }
  return "Unknown startup failure";
}

export function publishDesktopVisible() {
  if (window.__tandemDesktopVisible) {
    return;
  }
  window.__tandemDesktopVisible = true;
  window.dispatchEvent(new window.Event(DESKTOP_VISIBLE_EVENT));
}

export function reportStartupError(reason: unknown) {
  const message = toStartupMessage(reason);
  window.__tandemStartupError = message;
  window.dispatchEvent(
    new window.CustomEvent(STARTUP_ERROR_EVENT, {
      detail: {
        message,
      },
    })
  );
}
