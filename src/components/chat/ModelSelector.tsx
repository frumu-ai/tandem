import { useState, useRef, useEffect } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Check, ChevronDown, Search, Sparkles } from "lucide-react";
import { type ModelInfo, listModels, listOllamaModels, getProvidersConfig } from "@/lib/tauri";

interface ModelSelectorProps {
  currentModel?: string; // e.g. "gpt-4o"
  onModelSelect: (modelId: string, providerId: string) => void;
  className?: string;
  align?: "left" | "right";
  side?: "top" | "bottom";
}

interface GroupedModel {
  providerId: string;
  providerName: string;
  models: ModelInfo[];
}

export function ModelSelector({
  currentModel,
  onModelSelect,
  className,
  align = "left",
  side = "top",
}: ModelSelectorProps) {
  const [isOpen, setIsOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [groupedModels, setGroupedModels] = useState<GroupedModel[]>([]);
  const [selectedProviderFilter, setSelectedProviderFilter] = useState<"all" | string>("all");
  const [loading, setLoading] = useState(false);

  const containerRef = useRef<HTMLDivElement>(null);

  // Close when clicking outside
  useEffect(() => {
    const handleClickOutside = (event: any) => {
      if (containerRef.current && !containerRef.current.contains(event.target as any)) {
        setIsOpen(false);
      }
    };

    document.addEventListener("mousedown", handleClickOutside);
    return () => document.removeEventListener("mousedown", handleClickOutside);
  }, []);

  // Load models when opening
  useEffect(() => {
    if (isOpen) {
      loadModels();
    }
  }, [isOpen]);

  // Keep filter valid when provider list changes.
  useEffect(() => {
    if (selectedProviderFilter === "all") return;
    const stillExists = groupedModels.some((group) => group.providerId === selectedProviderFilter);
    if (!stillExists) {
      setSelectedProviderFilter("all");
    }
  }, [groupedModels, selectedProviderFilter]);

  const loadModels = async () => {
    setLoading(true);
    try {
      const [allModels, ollamaModels, config] = await Promise.all([
        listModels(),
        listOllamaModels().catch((err: unknown) => {
          console.warn("Failed to list Ollama models:", err);
          return [] as ModelInfo[];
        }),
        getProvidersConfig(), // We need this to check has_key
      ]);

      // Group by provider
      const groups: Record<string, GroupedModel> = {};

      // Helper to get friendly name
      const getProviderName = (id: string) => {
        switch (id) {
          case "openai":
            return "OpenAI";
          case "anthropic":
            return "Anthropic";
          case "openrouter":
            return "OpenRouter";
          case "opencode_zen":
            return "Opencode Zen";
          case "ollama":
            return "Ollama";
          case "poe":
            return "Poe";
          default:
            return id.charAt(0).toUpperCase() + id.slice(1);
        }
      };

      // Combine models (Ollama models might also be returned by sidecar if configured there, but we prioritize explicit list)
      const combinedModels = [...allModels];

      // Add discovered Ollama models if not already present
      ollamaModels.forEach((om: ModelInfo) => {
        // Check if we already have this model from sidecar
        const exists = combinedModels.some(
          (m) => (m.id === om.id || m.name === om.name) && m.provider === "ollama"
        );
        if (!exists) {
          combinedModels.push(om);
        }
      });

      // Add models from API
      combinedModels.forEach((model) => {
        // Normalize provider ID
        let providerId = model.provider || "unknown";
        if (providerId === "opencode") providerId = "opencode_zen";

        if (!groups[providerId]) {
          groups[providerId] = {
            providerId,
            providerName: getProviderName(providerId),
            models: [],
          };
        }
        groups[providerId].models.push(model);
      });

      setGroupedModels(
        Object.values(groups)
          // Trust sidecar catalog and show every provider/model it returns.
          // This matches OpenCode's "browse all models" behavior.
          .sort((a, b) => {
            // 1. OpenCode Zen always first
            if (a.providerId === "opencode_zen") return -1;
            if (b.providerId === "opencode_zen") return 1;

            // 2. Providers with keys configured come next (already filtered, but for ordering)
            const getConf = (pid: string) => {
              if (pid === "openai") return config.openai;
              if (pid === "anthropic") return config.anthropic;
              if (pid === "openrouter") return config.openrouter;
              if (pid === "opencode_zen") return config.opencode_zen;
              if (pid === "ollama") return config.ollama;
              if (pid === "poe") return config.poe;
              return undefined;
            };

            const confA = getConf(a.providerId);
            const confB = getConf(b.providerId);

            const hasKeyA = confA?.has_key || confA?.enabled;
            const hasKeyB = confB?.has_key || confB?.enabled;

            const isPriorityA = hasKeyA || a.providerId === "ollama";
            const isPriorityB = hasKeyB || b.providerId === "ollama";

            if (isPriorityA && !isPriorityB) return -1;
            if (!isPriorityA && isPriorityB) return 1;

            // 3. Alphabetical for the rest
            return a.providerName.localeCompare(b.providerName);
          })
      );
    } catch (e) {
      console.error("Failed to load models:", e);
    } finally {
      setLoading(false);
    }
  };

  const providerQueryMatch = search.match(/(?:^|\s)provider:([^\s]+)/i);
  const providerQueryValue = providerQueryMatch?.[1]?.trim().toLowerCase();
  const textSearch = search.replace(/(?:^|\s)provider:[^\s]+/gi, " ").trim();

  const resolvedProviderFromSearch =
    providerQueryValue &&
    groupedModels.find(
      (group) =>
        group.providerId.toLowerCase() === providerQueryValue ||
        group.providerName.toLowerCase() === providerQueryValue
    )?.providerId;

  const activeProviderFilter = resolvedProviderFromSearch || selectedProviderFilter;
  const isProviderForcedBySearch = Boolean(resolvedProviderFromSearch);

  const providerChips = [
    { id: "all", label: "All" },
    ...groupedModels.map((group) => ({ id: group.providerId, label: group.providerName })),
  ];

  const filteredGroups = groupedModels
    .filter((group) => activeProviderFilter === "all" || group.providerId === activeProviderFilter)
    .map((group) => ({
      ...group,
      models: group.models.filter(
        (m) =>
          m.name.toLowerCase().includes(textSearch.toLowerCase()) ||
          m.id.toLowerCase().includes(textSearch.toLowerCase())
      ),
    }))
    .filter((g) => g.models.length > 0);

  const activeProviderName =
    activeProviderFilter === "all"
      ? "All Providers"
      : groupedModels.find((group) => group.providerId === activeProviderFilter)?.providerName ||
        activeProviderFilter;

  // Find current model display name
  const currentModelDisplay = (() => {
    if (!currentModel) return "Select Model";
    // Try to find in loaded groups first if available
    for (const group of groupedModels) {
      const found = group.models.find((m) => m.id === currentModel);
      if (found) return found.name;
    }
    // Fallback to simple format
    return currentModel.split("/").pop() || currentModel;
  })();

  // Determine positioning classes
  const positionClasses = [
    "absolute",
    "z-50",
    side === "top" ? "bottom-full mb-2" : "top-full mt-2",
    align === "left" ? "left-0 origin-bottom-left" : "right-0 origin-bottom-right",
    "w-[300px]",
    "rounded-xl border border-border bg-surface shadow-xl overflow-hidden",
  ].join(" ");

  return (
    <div className={`relative ${className || ""}`} ref={containerRef}>
      <button
        onClick={() => setIsOpen(!isOpen)}
        className="flex items-center gap-2 rounded-lg border border-border/50 bg-surface px-3 py-1.5 text-xs hover:bg-surface-elevated transition-colors"
      >
        <div className="flex items-center gap-2">
          <Sparkles className="h-3.5 w-3.5 text-primary" />
          <span className="font-medium text-text max-w-[150px] truncate">
            {currentModelDisplay}
          </span>
        </div>
        <ChevronDown
          className={`h-3 w-3 text-text-subtle transition-transform ${isOpen ? "rotate-180" : ""}`}
        />
      </button>

      <AnimatePresence>
        {isOpen && (
          <motion.div
            initial={{ opacity: 0, y: side === "top" ? 10 : -10, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: side === "top" ? 10 : -10, scale: 0.98 }}
            transition={{ duration: 0.1 }}
            className={positionClasses}
          >
            <div className="p-2 border-b border-border bg-surface-elevated/30">
              <div className="relative">
                <Search className="absolute left-2.5 top-1/2 -translate-y-1/2 h-3.5 w-3.5 text-text-subtle" />
                <input
                  autoFocus
                  type="text"
                  placeholder="Search models..."
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  className="w-full rounded-lg bg-surface border border-border px-8 py-1.5 text-xs text-text placeholder:text-text-subtle focus:border-primary focus:outline-none"
                />
              </div>
              <div className="mt-2 text-[10px] text-text-subtle">
                Showing configured providers + local
              </div>
              <div className="mt-2">
                <div className="mb-1 text-[10px] text-text-subtle">Provider</div>
                <select
                  value={activeProviderFilter}
                  onChange={(e) => setSelectedProviderFilter(e.target.value)}
                  disabled={isProviderForcedBySearch}
                  className="w-full rounded-lg bg-surface border border-border px-2 py-1.5 text-xs text-text focus:border-primary focus:outline-none disabled:opacity-60"
                >
                  {providerChips.map((chip) => (
                    <option key={chip.id} value={chip.id}>
                      {chip.label}
                    </option>
                  ))}
                </select>
                {isProviderForcedBySearch && (
                  <div className="mt-1 text-[10px] text-text-subtle">
                    Provider locked by search token. Remove <code>provider:</code> to change.
                  </div>
                )}
              </div>
            </div>

            <div className="max-h-[300px] overflow-y-auto p-1">
              {loading ? (
                <div className="py-8 text-center text-xs text-text-subtle">Loading models...</div>
              ) : filteredGroups.length === 0 ? (
                <div className="py-8 text-center text-xs text-text-subtle">
                  {activeProviderFilter === "all"
                    ? "No models found"
                    : `No models found for ${activeProviderName}`}
                </div>
              ) : (
                <div className="space-y-1">
                  {filteredGroups.map((group) => (
                    <div key={group.providerId}>
                      <div className="px-2 py-1.5 text-[10px] font-bold uppercase tracking-wider text-text-subtle sticky top-0 bg-surface/95 backdrop-blur-sm">
                        {group.providerName}
                      </div>
                      {group.models.map((model) => (
                        <button
                          key={model.id}
                          onClick={() => {
                            onModelSelect(model.id, group.providerId);
                            setIsOpen(false);
                          }}
                          className={`flex w-full items-center justify-between rounded-md px-2 py-1.5 text-left text-xs transition-colors hover:bg-surface-elevated ${
                            currentModel === model.id ? "bg-primary/10 text-primary" : "text-text"
                          }`}
                        >
                          <span className="truncate">{model.name}</span>
                          {currentModel === model.id && <Check className="h-3 w-3" />}
                        </button>
                      ))}
                    </div>
                  ))}
                </div>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
