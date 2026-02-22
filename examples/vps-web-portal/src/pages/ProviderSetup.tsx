import React, { useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import { BrainCircuit, CheckCircle2, KeyRound, Loader2, Wrench } from "lucide-react";
import { api, type ProviderCatalog, type ProvidersConfigResponse } from "../api";
import { useAuth } from "../AuthContext";

interface ModelOption {
  id: string;
  name: string;
}

export const ProviderSetup: React.FC = () => {
  const navigate = useNavigate();
  const { refreshProviderStatus, providerConfigured } = useAuth();

  const [catalog, setCatalog] = useState<ProviderCatalog | null>(null);
  const [config, setConfig] = useState<ProvidersConfigResponse | null>(null);

  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState("");
  const [success, setSuccess] = useState("");

  const [providerId, setProviderId] = useState("");
  const [modelId, setModelId] = useState("");
  const [apiKey, setApiKey] = useState("");
  const [modelFilter, setModelFilter] = useState("");

  const providers = useMemo(() => {
    if (!catalog?.all) return [];
    return catalog.all.filter((p) => p.id !== "local");
  }, [catalog]);

  const models = useMemo<ModelOption[]>(() => {
    const provider = providers.find((p) => p.id === providerId);
    if (!provider?.models) return [];
    return Object.entries(provider.models)
      .map(([id, entry]) => ({
        id,
        name: entry.name || id,
      }))
      .sort((a, b) => a.name.localeCompare(b.name));
  }, [providers, providerId]);

  const filteredModels = useMemo(() => {
    if (!modelFilter.trim()) return models.slice(0, 200);
    const q = modelFilter.trim().toLowerCase();
    return models
      .filter((m) => m.name.toLowerCase().includes(q) || m.id.toLowerCase().includes(q))
      .slice(0, 200);
  }, [models, modelFilter]);

  const load = async () => {
    setLoading(true);
    setError("");
    try {
      const [providerCatalog, providerConfig] = await Promise.all([
        api.getProviderCatalog(),
        api.getProvidersConfig(),
      ]);
      setCatalog(providerCatalog);
      setConfig(providerConfig);

      const providerIds = providerCatalog.all.map((p) => p.id).filter((id) => id !== "local");
      const preferredProvider =
        (providerConfig.default && providerIds.includes(providerConfig.default)
          ? providerConfig.default
          : providerIds[0]) || "";
      setProviderId(preferredProvider);

      if (preferredProvider) {
        const configuredModel = providerConfig.providers?.[preferredProvider]?.default_model;
        if (configuredModel) {
          setModelId(configuredModel);
          setModelFilter(configuredModel);
        } else {
          const provider = providerCatalog.all.find((p) => p.id === preferredProvider);
          const firstModelId = provider?.models ? Object.keys(provider.models)[0] : "";
          setModelId(firstModelId);
          setModelFilter(firstModelId);
        }
      }
    } catch (e) {
      console.error(e);
      setError("Failed to load provider setup from engine.");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    if (providerConfigured) {
      navigate("/research", { replace: true });
      return;
    }
    load();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [providerConfigured]);

  useEffect(() => {
    if (!providerId) return;
    const configuredModel = config?.providers?.[providerId]?.default_model;
    if (configuredModel) {
      setModelId(configuredModel);
      setModelFilter(configuredModel);
      return;
    }
    if (!models.length) return;
    setModelId(models[0].id);
    setModelFilter(models[0].id);
  }, [providerId, config, models]);

  const handleSave = async (e: React.FormEvent) => {
    e.preventDefault();
    setError("");
    setSuccess("");

    if (!providerId) {
      setError("Select a provider.");
      return;
    }
    if (!modelId) {
      setError("Select a model.");
      return;
    }

    setSaving(true);
    try {
      if (apiKey.trim()) {
        await api.setProviderAuth(providerId, apiKey.trim());
      }

      await api.setProviderDefaults(providerId, modelId);
      await refreshProviderStatus();
      setSuccess("Provider and model saved.");
      navigate("/research", { replace: true });
    } catch (e) {
      console.error(e);
      setError("Failed to save provider settings. Verify key/model and try again.");
    } finally {
      setSaving(false);
    }
  };

  if (loading) {
    return (
      <div className="min-h-screen bg-gray-950 text-white flex items-center justify-center">
        <div className="flex items-center gap-2 text-gray-300">
          <Loader2 className="animate-spin" size={18} />
          Loading provider setup...
        </div>
      </div>
    );
  }

  if (!providers.length) {
    return (
      <div className="min-h-screen bg-gray-950 text-white flex items-center justify-center p-8">
        <div className="max-w-xl text-center">
          <h2 className="text-2xl font-bold mb-2">No External Providers Found</h2>
          <p className="text-gray-400">
            Engine currently reports no external providers. Configure providers in engine config or
            environment, then click Reload.
          </p>
          <button
            type="button"
            onClick={load}
            className="mt-4 px-4 py-2 text-sm rounded-md border border-gray-700 text-gray-300 hover:text-white hover:bg-gray-800"
          >
            Reload
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-950 flex flex-col justify-center py-12 sm:px-6 lg:px-8">
      <div className="sm:mx-auto sm:w-full sm:max-w-2xl">
        <div className="flex justify-center text-emerald-500">
          <BrainCircuit size={48} />
        </div>
        <h2 className="mt-6 text-center text-3xl font-extrabold text-white">
          Configure Provider & Model
        </h2>
        <p className="mt-2 text-center text-sm text-gray-400">
          First run setup: choose a provider, optionally add API key, and set default model.
        </p>
      </div>

      <div className="mt-8 sm:mx-auto sm:w-full sm:max-w-2xl">
        <div className="bg-gray-900 py-8 px-4 shadow-xl border border-gray-800 sm:rounded-lg sm:px-10">
          <form className="space-y-6" onSubmit={handleSave}>
            <div>
              <label className="block text-sm font-medium text-gray-300 mb-2">Provider</label>
              <select
                value={providerId}
                onChange={(e) => setProviderId(e.target.value)}
                className="w-full bg-gray-800 border border-gray-700 text-white rounded-md py-2 px-3 focus:ring-emerald-500 focus:border-emerald-500"
              >
                {providers.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name || p.id}
                  </option>
                ))}
              </select>
            </div>

            <div>
              <label className="block text-sm font-medium text-gray-300 mb-2">Model</label>
              <input
                type="text"
                value={modelFilter}
                onChange={(e) => setModelFilter(e.target.value)}
                placeholder="Filter models..."
                className="w-full mb-2 bg-gray-800 border border-gray-700 text-white rounded-md py-2 px-3"
              />
              <select
                value={modelId}
                onChange={(e) => setModelId(e.target.value)}
                className="w-full bg-gray-800 border border-gray-700 text-white rounded-md py-2 px-3"
              >
                {filteredModels.map((m) => (
                  <option key={m.id} value={m.id}>
                    {m.name}
                  </option>
                ))}
              </select>
              <p className="text-xs text-gray-500 mt-2">
                Showing up to 200 matching models for performance.
              </p>
            </div>

            <div>
              <label className="block text-sm font-medium text-gray-300 mb-2">
                Provider API Key (if required)
              </label>
              <div className="relative rounded-md shadow-sm">
                <div className="absolute inset-y-0 left-0 pl-3 flex items-center pointer-events-none">
                  <KeyRound className="h-5 w-5 text-gray-500" />
                </div>
                <input
                  type="password"
                  value={apiKey}
                  onChange={(e) => setApiKey(e.target.value)}
                  className="w-full pl-10 bg-gray-800 border border-gray-700 text-white rounded-md py-2 px-3"
                  placeholder="sk-... / or-... / provider key"
                />
              </div>
              <p className="text-xs text-gray-500 mt-2 flex items-start gap-2">
                <Wrench size={14} className="mt-0.5" />
                API keys are sent via engine runtime auth (`PUT /auth/provider`) and are not
                persisted by config patching.
              </p>
            </div>

            {error && <div className="text-red-400 text-sm font-medium">{error}</div>}
            {success && (
              <div className="text-emerald-400 text-sm font-medium flex items-center gap-2">
                <CheckCircle2 size={16} />
                {success}
              </div>
            )}

            <div className="flex gap-3">
              <button
                type="button"
                onClick={load}
                className="px-4 py-2 text-sm rounded-md border border-gray-700 text-gray-300 hover:text-white hover:bg-gray-800"
              >
                Reload
              </button>
              <button
                type="submit"
                disabled={saving}
                className="flex-1 py-2 px-4 rounded-md text-sm font-medium text-white bg-emerald-600 hover:bg-emerald-700 disabled:opacity-50"
              >
                {saving ? "Saving..." : "Save & Continue"}
              </button>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
};
