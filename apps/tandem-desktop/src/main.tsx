import { reportStartupError, publishDesktopVisible } from "@/lib/startupSignals";

declare global {
  interface Window {
    __tandemAppReady?: boolean;
    __tandemDesktopVisible?: boolean;
    __tandemStartupError?: string;
  }
}

window.addEventListener("error", (event) => {
  reportStartupError(event.error ?? event.message);
});

window.addEventListener("unhandledrejection", (event) => {
  reportStartupError(event.reason);
});

const rootElement = document.getElementById("root");

if (!rootElement) {
  reportStartupError("Missing #root container");
  throw new Error("Missing #root container");
}

const markVisibleIfRootPainted = () => {
  if (rootElement.childElementCount > 0 || rootElement.innerHTML.trim().length > 0) {
    publishDesktopVisible();
  }
};

const rootObserver = new window.MutationObserver(() => {
  markVisibleIfRootPainted();
  if (window.__tandemDesktopVisible) {
    rootObserver.disconnect();
  }
});

rootObserver.observe(rootElement, {
  childList: true,
  subtree: true,
  characterData: true,
});

window.requestAnimationFrame(() => {
  markVisibleIfRootPainted();
});
window.setTimeout(() => {
  markVisibleIfRootPainted();
}, 250);

void import("./app-entry").catch((error) => {
  console.error("[Startup] Failed to load desktop app bundle:", error);
  reportStartupError(error);
});
