import { Icon } from "../ui/Icon";

export const OPENAI_CODEX_PROVIDER_ID = "openai-codex";
export const OPENAI_CODEX_DEFAULT_MODEL_ID = "gpt-5.6-terra";

export function preferredProviderModel(providerId: string, models: string[], configuredModel = "") {
  const availableModels = mergeOptionValues(models, "");
  const configured = String(configuredModel || "").trim();
  if (configured) return configured;
  if (
    String(providerId || "")
      .trim()
      .toLowerCase() === OPENAI_CODEX_PROVIDER_ID
  ) {
    return (
      availableModels.find((model) => model === OPENAI_CODEX_DEFAULT_MODEL_ID) ||
      availableModels.find((model) => model.startsWith("gpt-5.6-")) ||
      availableModels[0] ||
      configured
    );
  }
  return availableModels[0] || configured;
}

type ProviderOption = {
  id: string;
  models: string[];
  configured?: boolean;
};

type ModelDraft = {
  provider: string;
  model: string;
};

function mergeOptionValues(values: string[], currentValue: string) {
  const seen = new Set<string>();
  const merged: string[] = [];
  for (const raw of [currentValue, ...values]) {
    const value = String(raw || "").trim();
    if (!value || seen.has(value)) continue;
    seen.add(value);
    merged.push(value);
  }
  return merged;
}

export function ProviderModelSelector({
  providerLabel,
  modelLabel,
  draft,
  providers,
  onChange,
  inheritLabel = "Workspace default",
  disabled = false,
}: {
  providerLabel: string;
  modelLabel: string;
  draft: ModelDraft;
  providers: ProviderOption[];
  onChange: (draft: ModelDraft) => void;
  inheritLabel?: string;
  disabled?: boolean;
}) {
  const modelOptions = providers.find((provider) => provider.id === draft.provider)?.models || [];
  const catalogModels = mergeOptionValues(modelOptions, "");
  const catalogModelSelected = catalogModels.includes(draft.model);
  return (
    <div className="grid gap-3 md:grid-cols-2">
      <label className="block text-sm">
        <div className="mb-1 flex items-center gap-2 font-medium text-slate-200">
          <Icon name="cpu" />
          <span>{providerLabel}</span>
        </div>
        <select
          value={draft.provider}
          onInput={(event) => {
            const provider = (event.target as HTMLSelectElement).value;
            const nextModels = providers.find((row) => row.id === provider)?.models || [];
            onChange({ provider, model: preferredProviderModel(provider, nextModels) });
          }}
          className="tcp-select h-10 w-full"
          disabled={disabled}
        >
          <option value="">{inheritLabel}</option>
          {mergeOptionValues(
            providers.map((provider) => provider.id),
            draft.provider
          ).map((providerId) => (
            <option key={providerId} value={providerId}>
              {providerId}
              {providers.find((provider) => provider.id === providerId)?.configured === false
                ? " (not configured)"
                : ""}
            </option>
          ))}
        </select>
      </label>
      <label className="block text-sm">
        <div className="mb-1 flex items-center gap-2 font-medium text-slate-200">
          <Icon name="sparkles" />
          <span>{modelLabel}</span>
        </div>
        {modelOptions.length ? (
          <div className="grid gap-2">
            <select
              aria-label={modelLabel}
              className="tcp-select h-10 w-full"
              value={catalogModelSelected ? draft.model : ""}
              onInput={(event) =>
                onChange({ ...draft, model: (event.target as HTMLSelectElement).value })
              }
              disabled={disabled || !draft.provider}
            >
              <option value="">Select a catalog model</option>
              {catalogModels.map((model) => (
                <option key={model} value={model}>
                  {model}
                </option>
              ))}
            </select>
            <input
              aria-label={`${modelLabel} custom ID`}
              className="tcp-input h-10 w-full"
              value={catalogModelSelected ? "" : draft.model}
              onInput={(event) =>
                onChange({ ...draft, model: (event.target as HTMLInputElement).value })
              }
              placeholder="Or type a custom model ID"
              disabled={disabled || !draft.provider}
            />
          </div>
        ) : (
          <input
            aria-label={modelLabel}
            className="tcp-input h-10 w-full"
            value={draft.model}
            onInput={(event) =>
              onChange({ ...draft, model: (event.target as HTMLInputElement).value })
            }
            placeholder={draft.provider ? "Type model ID" : inheritLabel}
            disabled={disabled || !draft.provider}
          />
        )}
      </label>
    </div>
  );
}
