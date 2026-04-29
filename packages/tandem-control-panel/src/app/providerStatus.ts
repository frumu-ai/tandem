export type ProviderState = {
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

function providerNeedsApiKey(providerId: string) {
  const id = String(providerId || "")
    .trim()
    .toLowerCase();
  return (
    !!id &&
    id !== "ollama" &&
    id !== "llama_cpp" &&
    id !== "llama.cpp" &&
    id !== "local" &&
    id !== "openai-codex"
  );
}

function providerHasUsableOAuth(value: any) {
  if (!value || typeof value !== "object") return false;
  const authKind = String(value.auth_kind || value.authKind || "")
    .trim()
    .toLowerCase();
  const status = String(value.status || "")
    .trim()
    .toLowerCase();
  if (authKind !== "oauth") return false;
  if (status === "reauth_required" || status === "expired" || status === "error") return false;
  return value.connected === true || status === "connected" || status === "configured";
}

function providerHasStoredKey(authStatus: any, providerId: string) {
  const id = String(providerId || "")
    .trim()
    .toLowerCase();
  if (!id || !authStatus || typeof authStatus !== "object") return false;

  const readCandidate = (value: any) => {
    if (!value || typeof value !== "object") return false;
    if (providerHasUsableOAuth(value)) return true;
    if (value.has_key === true || value.hasKey === true) return true;
    if (value.configured === true && !providerNeedsApiKey(id)) return true;
    return false;
  };

  return readCandidate(authStatus[id]) || readCandidate(authStatus.providers?.[id]);
}

function readProviderAuthEntry(authStatus: any, providerId: string) {
  const id = String(providerId || "")
    .trim()
    .toLowerCase();
  if (!id || !authStatus || typeof authStatus !== "object") return null;
  const direct = authStatus[id];
  if (direct && typeof direct === "object") return direct;
  const nested = authStatus.providers?.[id];
  if (nested && typeof nested === "object") return nested;
  return null;
}

export function deriveProviderState(config: any, catalog: any, authStatus: any): ProviderState {
  const defaultProvider = String(
    config?.default || config?.selected_model?.provider_id || ""
  ).trim();
  const providerConfig = config?.providers?.[defaultProvider] || {};
  const defaultModel = String(
    providerConfig.default_model ||
      providerConfig.defaultModel ||
      config?.selected_model?.model_id ||
      ""
  ).trim();
  const connected = new Set<string>();
  const addConnectedIds = (source: any) => {
    if (!source || typeof source !== "object") return;
    const rows =
      source.providers && typeof source.providers === "object" ? source.providers : source;
    if (!rows || typeof rows !== "object") return;
    for (const [providerId, value] of Object.entries(rows)) {
      if (!value || typeof value !== "object") continue;
      const normalizedId = String(providerId || "")
        .trim()
        .toLowerCase();
      if (!normalizedId) continue;
      const status = String((value as any).status || (value as any).statusText || "")
        .trim()
        .toLowerCase();
      if ((value as any).connected === true || status === "connected" || status === "configured") {
        connected.add(normalizedId);
      }
    }
  };
  if (Array.isArray(catalog?.connected)) {
    for (const id of catalog.connected) {
      const normalizedId = String(id || "")
        .trim()
        .toLowerCase();
      if (normalizedId) connected.add(normalizedId);
    }
  }
  addConnectedIds(authStatus);
  const authEntry = readProviderAuthEntry(authStatus, defaultProvider);
  const hasStoredKey = providerHasStoredKey(authStatus, defaultProvider);
  const ready =
    !!defaultProvider && !!defaultModel && (!providerNeedsApiKey(defaultProvider) || hasStoredKey);

  return {
    ready,
    defaultProvider,
    defaultModel,
    connected: [...connected],
    error: "",
    needsOnboarding: !ready,
    defaultProviderAuthKind: String(authEntry?.auth_kind || authEntry?.authKind || "")
      .trim()
      .toLowerCase(),
    defaultProviderSource: String(authEntry?.source || "")
      .trim()
      .toLowerCase(),
    defaultProviderManagedBy: String(authEntry?.managed_by || authEntry?.managedBy || "")
      .trim()
      .toLowerCase(),
  };
}
