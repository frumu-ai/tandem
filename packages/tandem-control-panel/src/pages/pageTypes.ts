import type { TandemClient } from "@frumu/tandem-client";
import type { RouteId } from "../app/routes";

export type ToastKind = "ok" | "info" | "warn" | "err";

export type ProviderStatus = {
  ready: boolean;
  defaultProvider: string;
  defaultModel: string;
  connected: string[];
  error: string;
  needsOnboarding: boolean;
};

export type IdentityInfo = {
  botName: string;
  botAvatarUrl: string;
  controlPanelName: string;
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
};
