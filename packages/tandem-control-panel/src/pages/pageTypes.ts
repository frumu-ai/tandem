import type { TandemClient } from "@frumu/tandem-client";
import type { RouteId } from "../app/routes";
import type { NavigationVisibility } from "../app/navigation";

export type ToastKind = "ok" | "info" | "warn" | "err";

export type ProviderStatus = {
  ready: boolean;
  defaultProvider: string;
  defaultModel: string;
  connected: string[];
  error: string;
  needsOnboarding: boolean;
  defaultProviderAuthKind: string;
  defaultProviderSource: string;
  defaultProviderManagedBy: string;
};

export type IdentityInfo = {
  botName: string;
  botAvatarUrl: string;
  controlPanelName: string;
};

export type NavigationLockState = {
  title: string;
  message: string;
};

export type NavigationPreferences = {
  acaMode: boolean;
  routeVisibility: NavigationVisibility;
  setRouteVisibility: (routeId: RouteId, visible: boolean) => void;
  showAllSections: () => void;
  resetNavigation: () => void;
};

export type AppPageProps = {
  path?: string;
  default?: boolean;
  client: TandemClient;
  api: (path: string, init?: RequestInit) => Promise<any>;
  toast: (kind: ToastKind, text: string) => void;
  navigate: (route: string) => void;
  currentRoute: RouteId;
  providerStatus: ProviderStatus;
  identity: IdentityInfo;
  refreshProviderStatus: () => Promise<void>;
  refreshIdentityStatus: () => Promise<void>;
  themes: any[];
  setTheme: (themeId: string) => any;
  themeId: string;
  navigation?: NavigationPreferences;
  navigationLock?: NavigationLockState | null;
  setNavigationLock?: (lock: NavigationLockState | null) => void;
};
