import { Icon } from "../ui/Icon";
import { SearchInput } from "../ui/index.tsx";
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
  const filteredModels = mergeOptionValues(modelOptions, draft.model)
    .filter((model) => {
      const query = String(draft.model || "")
        .trim()
        .toLowerCase();
      return !query || model.toLowerCase().includes(query);
    })
    .slice(0, 80);
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
            onChange({ provider, model: nextModels[0] || "" });
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
        <SearchInput
          aria-label="Filter models"
          className="tcp-input h-10 w-full"
          value={draft.model}
          onInput={(event) =>
            onChange({ ...draft, model: (event.target as HTMLInputElement).value })
          }
          placeholder={draft.provider ? "Type to filter models" : inheritLabel}
          disabled={disabled || !draft.provider}
        />
        <div className="mt-2 max-h-48 overflow-auto rounded-xl border border-slate-700/60 bg-slate-900/20 p-1">
          {draft.provider ? (
            filteredModels.length ? (
              filteredModels.map((model) => (
                <button
                  key={model}
                  type="button"
                  className={`block w-full rounded-lg px-2 py-1.5 text-left text-sm hover:bg-slate-700/30 ${
                    model === draft.model ? "bg-slate-700/40" : ""
                  }`}
                  onClick={() => onChange({ ...draft, model })}
                  disabled={disabled}
                >
                  {model}
                </button>
              ))
            ) : (
              <div className="tcp-subtle px-2 py-1 text-xs">
                No matching models. Type a model id manually.
              </div>
            )
          ) : (
            <div className="tcp-subtle px-2 py-1 text-xs">{inheritLabel}</div>
          )}
        </div>
      </label>
    </div>
  );
}
