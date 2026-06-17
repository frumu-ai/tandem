import type { Dispatch, SetStateAction } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { useTranslation } from "react-i18next";
import { Cpu } from "lucide-react";
import { ProviderCard } from "./ProviderCard";
import { Button } from "@/components/ui/Button";
import { Input } from "@/components/ui/Input";
import { Switch } from "@/components/ui/Switch";
import { Card, CardHeader, CardTitle, CardDescription, CardContent } from "@/components/ui/Card";
import type { ProvidersConfig, SchedulerSettings, SearchSettings } from "@/lib/tauri";
import { PROVIDER_CARD_DEFINITIONS, type ProviderSlot } from "./providerSettingsModel";

type Notice = { kind: "success" | "error"; message: string } | null;

interface ProvidersSettingsProps {
  activeTab: "settings" | "providers" | "connections" | "logs";
  providers: ProvidersConfig | null;
  providerCatalogModels: Record<string, string[]>;
  customEnabled: boolean;
  customEndpoint: string;
  customModel: string;
  customApiKey: string;
  customProviderNotice: Notice;
  searchSettingsState: SearchSettings | null;
  searchBraveKeyInput: string;
  searchExaKeyInput: string;
  searchHasBraveKey: boolean;
  searchHasExaKey: boolean;
  searchSaving: boolean;
  searchNotice: Notice;
  schedulerSettingsState: SchedulerSettings | null;
  onProviderChange?: () => void;
  handleProviderChange: (provider: ProviderSlot, enabled: boolean) => Promise<void>;
  handleSetDefault: (provider: ProviderSlot) => Promise<void>;
  handleModelChange: (provider: ProviderSlot, model: string) => Promise<void>;
  handleEndpointChange: (provider: ProviderSlot, endpoint: string) => Promise<void>;
  handleCustomProviderToggle: (enabled: boolean) => Promise<void>;
  handleCustomProviderSave: () => Promise<void>;
  handleSearchSettingsSave: () => Promise<void>;
  handleSearchKeySave: (keyType: "brave_search" | "exa_search") => Promise<void>;
  handleSearchKeyDelete: (keyType: "brave_search" | "exa_search") => Promise<void>;
  handleSchedulerSettingsSave: () => Promise<void>;
  setCustomEndpoint: Dispatch<SetStateAction<string>>;
  setCustomModel: Dispatch<SetStateAction<string>>;
  setCustomApiKey: Dispatch<SetStateAction<string>>;
  setSearchSettingsState: Dispatch<SetStateAction<SearchSettings | null>>;
  setSearchBraveKeyInput: Dispatch<SetStateAction<string>>;
  setSearchExaKeyInput: Dispatch<SetStateAction<string>>;
  setSchedulerSettingsState: Dispatch<SetStateAction<SchedulerSettings | null>>;
}

export function ProvidersSettings({
  activeTab,
  providers,
  providerCatalogModels,
  customEnabled,
  customEndpoint,
  customModel,
  customApiKey,
  customProviderNotice,
  searchSettingsState,
  searchBraveKeyInput,
  searchExaKeyInput,
  searchHasBraveKey,
  searchHasExaKey,
  searchSaving,
  searchNotice,
  schedulerSettingsState,
  onProviderChange,
  handleProviderChange,
  handleSetDefault,
  handleModelChange,
  handleEndpointChange,
  handleCustomProviderToggle,
  handleCustomProviderSave,
  handleSearchSettingsSave,
  handleSearchKeySave,
  handleSearchKeyDelete,
  handleSchedulerSettingsSave,
  setCustomEndpoint,
  setCustomModel,
  setCustomApiKey,
  setSearchSettingsState,
  setSearchBraveKeyInput,
  setSearchExaKeyInput,
  setSchedulerSettingsState,
}: ProvidersSettingsProps) {
  const { t } = useTranslation(["settings"]);

  return (
    <div
      className={`providers-settings-section space-y-4 ${activeTab === "settings" ? "hidden" : ""}`}
    >
      <div className="flex items-center gap-3">
        <Cpu className="h-5 w-5 text-primary" />
        <div>
          <h2 className="text-lg font-semibold text-text">
            {t("providers.title", { ns: "settings" })}
          </h2>
          <p className="text-sm text-text-muted">
            {t("providersPanel.description", { ns: "settings" })}
          </p>
        </div>
      </div>

      {providers && (
        <div className="space-y-4">
          {PROVIDER_CARD_DEFINITIONS.map((definition) => {
            const provider = providers[definition.slot];
            return (
              <ProviderCard
                key={definition.slot}
                id={definition.slot}
                name={definition.name}
                description={t(`providersCatalog.${definition.slot}.description`, {
                  ns: "settings",
                })}
                endpoint={provider.endpoint}
                defaultEndpoint={definition.defaultEndpoint}
                model={provider.model}
                catalogModelIds={providerCatalogModels[definition.slot] ?? []}
                isDefault={provider.default}
                enabled={provider.enabled}
                onEnabledChange={(enabled) => handleProviderChange(definition.slot, enabled)}
                onModelChange={(model) => handleModelChange(definition.slot, model)}
                onEndpointChange={(endpoint) => handleEndpointChange(definition.slot, endpoint)}
                onSetDefault={() => handleSetDefault(definition.slot)}
                onKeyChange={onProviderChange}
                docsUrl={definition.docsUrl}
                supportsOAuth={definition.supportsOAuth}
              />
            );
          })}

          {searchSettingsState && (
            <Card>
              <CardHeader>
                <CardTitle>Web Search</CardTitle>
                <CardDescription>
                  Choose the search backend for `websearch` and store Brave or Exa keys in the
                  encrypted vault.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="grid gap-4 md:grid-cols-2">
                  <div className="space-y-2">
                    <label className="text-xs font-medium text-text-subtle">Backend</label>
                    <select
                      className="w-full rounded-xl border border-border bg-surface px-3 py-2 text-sm text-text focus:outline-none focus:ring-2 focus:ring-primary/40"
                      value={searchSettingsState.backend}
                      onChange={(e) =>
                        setSearchSettingsState((prev) =>
                          prev ? { ...prev, backend: e.target.value } : prev
                        )
                      }
                    >
                      <option value="auto">Auto failover</option>
                      <option value="brave">Brave Search</option>
                      <option value="exa">Exa</option>
                      <option value="searxng">SearxNG</option>
                      <option value="tandem">Tandem hosted search</option>
                      <option value="none">Disable websearch</option>
                    </select>
                  </div>
                  <Input
                    label="Timeout (ms)"
                    type="number"
                    min={1000}
                    max={120000}
                    value={String(searchSettingsState.timeout_ms ?? 10000)}
                    onChange={(e) =>
                      setSearchSettingsState((prev) =>
                        prev
                          ? {
                              ...prev,
                              timeout_ms: Number.parseInt(e.target.value || "10000", 10),
                            }
                          : prev
                      )
                    }
                  />
                </div>

                <div className="grid gap-4 md:grid-cols-2">
                  <Input
                    label="Tandem search URL"
                    placeholder="https://search.tandem.ac"
                    value={searchSettingsState.tandem_url ?? ""}
                    onChange={(e) =>
                      setSearchSettingsState((prev) =>
                        prev ? { ...prev, tandem_url: e.target.value } : prev
                      )
                    }
                  />
                  <Input
                    label="SearxNG URL"
                    placeholder="http://127.0.0.1:8080"
                    value={searchSettingsState.searxng_url ?? ""}
                    onChange={(e) =>
                      setSearchSettingsState((prev) =>
                        prev ? { ...prev, searxng_url: e.target.value } : prev
                      )
                    }
                  />
                </div>

                <div className="grid gap-4 md:grid-cols-2">
                  <div className="space-y-2 rounded-xl border border-border bg-surface-elevated/40 p-4">
                    <div className="flex items-center justify-between gap-3">
                      <div>
                        <p className="text-sm font-medium text-text">Brave Search key</p>
                        <p className="text-xs text-text-muted">
                          Best for fast live lookups. Stored in the encrypted vault.
                        </p>
                      </div>
                      <span className="text-xs text-text-muted">
                        {searchHasBraveKey ? "Saved" : "Not saved"}
                      </span>
                    </div>
                    <Input
                      type="password"
                      placeholder="BSA..."
                      value={searchBraveKeyInput}
                      onChange={(e) => setSearchBraveKeyInput(e.target.value)}
                    />
                    <div className="flex flex-wrap gap-2">
                      <Button
                        variant="secondary"
                        onClick={() => handleSearchKeySave("brave_search")}
                        disabled={searchSaving || !searchBraveKeyInput.trim()}
                      >
                        Save Brave Key
                      </Button>
                      {searchHasBraveKey && (
                        <Button
                          variant="ghost"
                          onClick={() => handleSearchKeyDelete("brave_search")}
                          disabled={searchSaving}
                        >
                          Remove
                        </Button>
                      )}
                    </div>
                  </div>

                  <div className="space-y-2 rounded-xl border border-border bg-surface-elevated/40 p-4">
                    <div className="flex items-center justify-between gap-3">
                      <div>
                        <p className="text-sm font-medium text-text">Exa key</p>
                        <p className="text-xs text-text-muted">
                          Useful as a fallback or primary search backend. Stored in the encrypted
                          vault.
                        </p>
                      </div>
                      <span className="text-xs text-text-muted">
                        {searchHasExaKey ? "Saved" : "Not saved"}
                      </span>
                    </div>
                    <Input
                      type="password"
                      placeholder="Paste Exa API key"
                      value={searchExaKeyInput}
                      onChange={(e) => setSearchExaKeyInput(e.target.value)}
                    />
                    <div className="flex flex-wrap gap-2">
                      <Button
                        variant="secondary"
                        onClick={() => handleSearchKeySave("exa_search")}
                        disabled={searchSaving || !searchExaKeyInput.trim()}
                      >
                        Save Exa Key
                      </Button>
                      {searchHasExaKey && (
                        <Button
                          variant="ghost"
                          onClick={() => handleSearchKeyDelete("exa_search")}
                          disabled={searchSaving}
                        >
                          Remove
                        </Button>
                      )}
                    </div>
                  </div>
                </div>

                <div className="flex flex-wrap items-center gap-3">
                  <Button onClick={handleSearchSettingsSave} disabled={searchSaving}>
                    {searchSaving ? "Saving..." : "Save Search Settings"}
                  </Button>
                  {searchNotice && (
                    <p
                      className={
                        searchNotice.kind === "success"
                          ? "text-sm text-emerald-400"
                          : "text-sm text-rose-400"
                      }
                    >
                      {searchNotice.message}
                    </p>
                  )}
                </div>

                <p className="text-xs text-text-muted">
                  `auto` will try the configured backends with failover. If Brave is rate-limited,
                  Tandem can continue with Exa when both keys are present.
                </p>
              </CardContent>
            </Card>
          )}

          {schedulerSettingsState && (
            <Card>
              <CardHeader>
                <CardTitle>Automation Scheduler</CardTitle>
                <CardDescription>
                  Controls whether automation runs are executed one at a time or in parallel.
                </CardDescription>
              </CardHeader>
              <CardContent className="space-y-4">
                <div className="grid gap-4 md:grid-cols-2">
                  <div className="space-y-2">
                    <label className="text-xs font-medium text-text-subtle">Mode</label>
                    <select
                      className="w-full rounded-xl border border-border bg-surface px-3 py-2 text-sm text-text focus:outline-none focus:ring-2 focus:ring-primary/40"
                      value={schedulerSettingsState.mode}
                      onChange={(e) =>
                        setSchedulerSettingsState((prev) =>
                          prev ? { ...prev, mode: e.target.value } : prev
                        )
                      }
                    >
                      <option value="multi">Multi - parallel runs (recommended)</option>
                      <option value="single">Single - one run at a time</option>
                    </select>
                  </div>
                  <Input
                    label="Max concurrent runs"
                    type="number"
                    min={1}
                    max={32}
                    value={
                      schedulerSettingsState.max_concurrent_runs != null
                        ? String(schedulerSettingsState.max_concurrent_runs)
                        : ""
                    }
                    placeholder="8 (default)"
                    onChange={(e) =>
                      setSchedulerSettingsState((prev) =>
                        prev
                          ? {
                              ...prev,
                              max_concurrent_runs: e.target.value
                                ? Number.parseInt(e.target.value, 10)
                                : null,
                            }
                          : prev
                      )
                    }
                  />
                </div>
                <div className="flex flex-wrap items-center gap-3">
                  <Button onClick={handleSchedulerSettingsSave}>Save Scheduler Settings</Button>
                </div>
                <p className="text-xs text-text-muted">
                  Multi mode allows several automation runs to execute concurrently. Max concurrent
                  runs caps parallelism. Changes take effect on the next engine start.
                </p>
              </CardContent>
            </Card>
          )}

          <Card className="border-2 border-dashed">
            <CardHeader>
              <div className="flex items-center justify-between">
                <div>
                  <CardTitle>
                    {t("providersPanel.customProviderTitle", { ns: "settings" })}
                  </CardTitle>
                  <CardDescription>
                    {t("providersPanel.customProviderDescription", { ns: "settings" })}
                  </CardDescription>
                </div>
                <Switch
                  checked={customEnabled}
                  onChange={(e) => handleCustomProviderToggle(e.target.checked)}
                />
              </div>
            </CardHeader>
            <AnimatePresence>
              {customEnabled && (
                <motion.div
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: "auto", opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                  transition={{ duration: 0.2 }}
                >
                  <CardContent className="space-y-4">
                    <div>
                      <label className="text-xs font-medium text-text-subtle">
                        {t("providersPanel.endpointUrl", { ns: "settings" })}
                      </label>
                      <Input
                        placeholder={t("providers.endpointPlaceholder", { ns: "settings" })}
                        value={customEndpoint}
                        onChange={(e) => setCustomEndpoint(e.target.value)}
                      />
                    </div>
                    <div>
                      <label className="text-xs font-medium text-text-subtle">
                        {t("providers.model", { ns: "settings" })}
                      </label>
                      <Input
                        placeholder={t("providersPanel.modelPlaceholder", { ns: "settings" })}
                        value={customModel}
                        onChange={(e) => setCustomModel(e.target.value)}
                      />
                    </div>
                    <div>
                      <label className="text-xs font-medium text-text-subtle">
                        {t("providersPanel.apiKeyOptional", { ns: "settings" })}
                      </label>
                      <Input
                        type="password"
                        placeholder="sk-..."
                        value={customApiKey}
                        onChange={(e) => setCustomApiKey(e.target.value)}
                      />
                    </div>
                    <Button
                      onClick={handleCustomProviderSave}
                      disabled={!customEndpoint.trim()}
                      className="w-full"
                    >
                      {t("providersPanel.saveCustomProvider", { ns: "settings" })}
                    </Button>
                    {customProviderNotice && (
                      <p
                        className={
                          customProviderNotice.kind === "success"
                            ? "text-xs text-success"
                            : "text-xs text-error"
                        }
                      >
                        {customProviderNotice.message}
                      </p>
                    )}
                  </CardContent>
                </motion.div>
              )}
            </AnimatePresence>
          </Card>
        </div>
      )}
    </div>
  );
}
