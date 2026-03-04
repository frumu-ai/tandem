import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

export function SettingsPage({
  client,
  toast,
  themes,
  setTheme,
  themeId,
  refreshProviderStatus,
  refreshIdentityStatus,
}: AppPageProps) {
  const queryClient = useQueryClient();
  const providersCatalog = useQuery({
    queryKey: ["settings", "providers", "catalog"],
    queryFn: () => client.providers.catalog().catch(() => ({ all: [], connected: [] })),
  });
  const providersConfig = useQuery({
    queryKey: ["settings", "providers", "config"],
    queryFn: () => client.providers.config().catch(() => ({ default: "", providers: {} })),
  });
  const [selectedProvider, setSelectedProvider] = [
    String(providersConfig.data?.default || ""),
    () => undefined,
  ] as any;

  const setDefaultsMutation = useMutation({
    mutationFn: async ({ providerId, modelId }: { providerId: string; modelId: string }) =>
      client.providers.setDefaults(providerId, modelId),
    onSuccess: async () => {
      toast("ok", "Updated provider defaults.");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["settings", "providers"] }),
        refreshProviderStatus(),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const setApiKeyMutation = useMutation({
    mutationFn: ({ providerId, apiKey }: { providerId: string; apiKey: string }) =>
      client.providers.setApiKey(providerId, apiKey),
    onSuccess: async () => {
      toast("ok", "API key updated.");
      await refreshProviderStatus();
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const providers = Array.isArray(providersCatalog.data?.all) ? providersCatalog.data.all : [];

  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <PageCard title="Provider Setup" subtitle="Default provider/model and API keys">
        <div className="grid gap-2">
          {providers.length ? (
            providers.map((provider: any) => {
              const providerId = String(provider?.id || "");
              const models = Object.keys(provider?.models || {});
              const defaultModel = String(
                providersConfig.data?.providers?.[providerId]?.default_model || models[0] || ""
              );
              return (
                <details key={providerId} className="tcp-list-item">
                  <summary className="cursor-pointer font-medium">{providerId}</summary>
                  <div className="mt-2 grid gap-2">
                    <select
                      className="tcp-select"
                      defaultValue={defaultModel}
                      onChange={(e) =>
                        setDefaultsMutation.mutate({
                          providerId,
                          modelId: (e.target as HTMLSelectElement).value,
                        })
                      }
                    >
                      {models.map((modelId) => (
                        <option key={modelId} value={modelId}>
                          {modelId}
                        </option>
                      ))}
                    </select>
                    <form
                      onSubmit={(e) => {
                        e.preventDefault();
                        const input = e.currentTarget.elements.namedItem(
                          "apiKey"
                        ) as HTMLInputElement;
                        const value = String(input?.value || "").trim();
                        if (!value) return;
                        setApiKeyMutation.mutate({ providerId, apiKey: value });
                        input.value = "";
                      }}
                      className="flex gap-2"
                    >
                      <input
                        name="apiKey"
                        className="tcp-input"
                        placeholder={`Set ${providerId} API key`}
                      />
                      <button className="tcp-btn" type="submit">
                        Save
                      </button>
                    </form>
                  </div>
                </details>
              );
            })
          ) : (
            <EmptyState text="No providers detected." />
          )}
        </div>
      </PageCard>

      <PageCard title="Theme + Identity" subtitle="Control panel look and identity refresh">
        <div className="grid gap-3">
          <label className="text-sm tcp-subtle">Theme</label>
          <select
            className="tcp-select"
            value={themeId}
            onChange={(e) => setTheme((e.target as HTMLSelectElement).value)}
          >
            {themes.map((theme: any) => (
              <option key={theme.id} value={theme.id}>
                {theme.name}
              </option>
            ))}
          </select>
          <button
            className="tcp-btn"
            onClick={() => refreshIdentityStatus().then(() => toast("ok", "Identity refreshed."))}
          >
            Refresh Identity
          </button>
          <button
            className="tcp-btn"
            onClick={() =>
              refreshProviderStatus().then(() => toast("ok", "Provider status refreshed."))
            }
          >
            Refresh Provider Status
          </button>
          <div className="tcp-list-item">
            <div className="text-sm">
              Connected providers: {String(providersCatalog.data?.connected?.length || 0)}
            </div>
            <div className="tcp-subtle text-xs">
              Default: {String(providersConfig.data?.default || "none")}
            </div>
          </div>
        </div>
      </PageCard>
    </div>
  );
}
