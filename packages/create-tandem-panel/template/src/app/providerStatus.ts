export type ProviderState = {
  ready: boolean;
  defaultProvider: string;
  defaultModel: string;
  connected: string[];
  error: string;
  needsOnboarding: boolean;
};

function providerNeedsApiKey(providerId: string) {
  const id = String(providerId || "")
    .trim()
    .toLowerCase();
  return !!id && id !== "ollama" && id !== "local";
}

function providerHasStoredKey(authStatus: any, providerId: string) {
  const id = String(providerId || "")
    .trim()
    .toLowerCase();
  if (!id || !authStatus || typeof authStatus !== "object") return false;

  const readCandidate = (value: any) => {
    if (!value || typeof value !== "object") return false;
    if (value.has_key === true || value.hasKey === true) return true;
    if (value.configured === true && !providerNeedsApiKey(id)) return true;
    return false;
  };

  return readCandidate(authStatus[id]) || readCandidate(authStatus.providers?.[id]);
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
  const connectedProviders = Array.isArray(catalog?.connected) ? catalog.connected : [];
  const connected = new Set<string>(
    connectedProviders.map((id: any) =>
      String(id || "")
        .trim()
        .toLowerCase()
    )
  );
  const hasStoredKey = providerHasStoredKey(authStatus, defaultProvider);
  const ready =
    !!defaultProvider &&
    !!defaultModel &&
    connected.has(defaultProvider.toLowerCase()) &&
    (!providerNeedsApiKey(defaultProvider) || hasStoredKey);

  return {
    ready,
    defaultProvider,
    defaultModel,
    connected: [...connected],
    error: "",
    needsOnboarding: !ready,
  };
}
