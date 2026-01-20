import React, { createContext, useContext, useState, useEffect, useCallback } from "react";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { relaunch } from "@tauri-apps/plugin-process";

export type UpdateStatus =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "installed"
  | "upToDate"
  | "error";

interface UpdaterContextType {
  status: UpdateStatus;
  updateInfo: Update | null;
  error: string | null;
  checkUpdates: (silent?: boolean) => Promise<void>;
  installUpdate: () => Promise<void>;
  dismissUpdate: () => void;
}

const UpdaterContext = createContext<UpdaterContextType | null>(null);

export function UpdaterProvider({ children }: { children: React.ReactNode }) {
  const [status, setStatus] = useState<UpdateStatus>("idle");
  const [updateInfo, setUpdateInfo] = useState<Update | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [isDismissed, setIsDismissed] = useState(false);

  const checkUpdates = useCallback(async (silent = false) => {
    if (!silent) {
      setStatus("checking");
    }
    setError(null);

    try {
      const update = await check();
      if (!update) {
        if (!silent) setStatus("upToDate");
        setUpdateInfo(null);
        return;
      }

      console.log(`Update available: ${update.version}`);
      setUpdateInfo(update);
      setStatus("available");
      setIsDismissed(false); // Reset dismiss on new check finding an update
    } catch (err) {
      console.error("Update check failed:", err);
      if (!silent) {
        setStatus("error");
        setError(err instanceof Error ? err.message : "Update check failed.");
      }
    }
  }, []);

  const installUpdate = useCallback(async () => {
    if (!updateInfo) return;

    setStatus("downloading");
    setError(null);

    try {
      await updateInfo.downloadAndInstall();
      setStatus("installed");
      await relaunch();
    } catch (err) {
      console.error("Update install failed:", err);
      setStatus("error");
      setError(err instanceof Error ? err.message : "Update install failed.");
    }
  }, [updateInfo]);

  const dismissUpdate = useCallback(() => {
    setIsDismissed(true);
  }, []);

  // Check for updates on mount
  useEffect(() => {
    checkUpdates(true);
  }, [checkUpdates]);

  // If dismissed, effectively hide it from the UI consumers (unless they explicitly check status)
  // But for the About page, we might want to still show it.
  // The consumer can decide how to handle "dismissed".
  // Actually, let's just expose isDismissed? No, let's keep it simple.
  // We'll expose `dismissUpdate` and let consumers filter if they want.
  // But wait, if About page uses this, dismissing the Toast shouldn't hide the "Update Available" in About.
  // So `isDismissed` should be a separate state exposed to consumers, OR consumers handle their own visibility.
  // Better: Expose `isDismissed` so the Toast can use it.

  return (
    <UpdaterContext.Provider
      value={{
        status,
        updateInfo,
        error,
        checkUpdates,
        installUpdate,
        dismissUpdate,
        // @ts-ignore - appending hidden prop for Toast usage
        isDismissed,
      }}
    >
      {children}
    </UpdaterContext.Provider>
  );
}

export function useUpdater() {
  const context = useContext(UpdaterContext);
  if (!context) {
    throw new Error("useUpdater must be used within an UpdaterProvider");
  }
  return context as UpdaterContextType & { isDismissed: boolean };
}
