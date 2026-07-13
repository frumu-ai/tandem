import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "preact/hooks";
import { Icon } from "../../ui/Icon";

const DATA_CLASS_OPTIONS = [
  "public",
  "internal",
  "confidential",
  "restricted",
  "customer_data",
  "source_code",
  "financial_record",
  "credential",
  "regulated",
  "executive",
];

const RISK_OPTIONS = [
  "",
  "read_discover",
  "internal_write",
  "consequential_write",
  "external_draft",
  "external_send",
  "customer_data_access",
  "source_code_mutation",
  "financial_record_access",
  "credential_admin",
  "destructive_delete",
  "money_movement_contract",
];

const LINEAR_PROVIDER = "linear";
const LINEAR_SIGNATURE_SCHEME = "linear_hmac_sha256";
const NOTION_SIGNATURE_SCHEME = "notion_hmac_sha256";
const UNSIGNED_DEV_MODE_SCHEME = "unsigned_dev_mode";

const SIGNATURE_SCHEME_OPTIONS = [
  { value: "", label: "Provider default" },
  { value: "hmac_sha256_v1", label: "Tandem HMAC SHA-256" },
  { value: "github_hmac_sha256", label: "GitHub HMAC SHA-256" },
  { value: LINEAR_SIGNATURE_SCHEME, label: "Linear HMAC SHA-256" },
  { value: NOTION_SIGNATURE_SCHEME, label: "Notion HMAC SHA-256" },
  { value: "shared_secret_header_v1", label: "Shared secret header" },
  { value: UNSIGNED_DEV_MODE_SCHEME, label: "Unsigned dev mode" },
];

function safeString(value: unknown) {
  return typeof value === "string" ? value.trim() : "";
}

function canonicalProvider(value: unknown) {
  const normalized = safeString(value).toLowerCase();
  if (normalized === "linear.app") return LINEAR_PROVIDER;
  if (normalized === "notion.so" || normalized === "notion.com") return "notion";
  if (normalized === "github.com" || normalized === "gh") return "github";
  return normalized;
}

function triggerId(trigger: any) {
  return safeString(trigger?.trigger_id || trigger?.triggerID);
}

function deliveryId(delivery: any) {
  return safeString(delivery?.delivery_id || delivery?.deliveryID);
}

function callbackUrl(trigger: any) {
  return safeString(
    trigger?.callback_url || trigger?.callbackUrl || trigger?.callback_path || trigger?.callbackPath
  );
}

function triggerProvider(trigger: any) {
  return canonicalProvider(
    trigger?.provider_metadata?.canonical_provider ||
      trigger?.providerMetadata?.canonicalProvider ||
      trigger?.provider
  );
}

function isLinearProvider(value: unknown) {
  return canonicalProvider(value) === LINEAR_PROVIDER;
}

function signatureScheme(trigger: any) {
  return safeString(
    trigger?.signature_scheme ||
      trigger?.signatureScheme ||
      trigger?.provider_metadata?.verification?.signature_scheme ||
      trigger?.providerMetadata?.verification?.signatureScheme
  );
}

function recommendedSignatureScheme(provider: unknown, current = "") {
  if (isLinearProvider(provider)) return LINEAR_SIGNATURE_SCHEME;
  if (canonicalProvider(provider) === "notion") return NOTION_SIGNATURE_SCHEME;
  return current;
}

function secretStatus(trigger: any) {
  const raw = trigger?.secret_status || trigger?.secretStatus;
  return raw && typeof raw === "object" ? raw : {};
}

function linearSecretConfigured(trigger: any) {
  const providerStatus =
    trigger?.provider_secret_status ||
    trigger?.providerSecretStatus ||
    trigger?.linear_secret_status ||
    trigger?.linearSecretStatus;
  if (providerStatus && typeof providerStatus === "object") {
    return providerStatus.configured === true || providerStatus.secret_configured === true;
  }
  if (
    trigger?.provider_secret_configured === true ||
    trigger?.providerSecretConfigured === true ||
    trigger?.linear_secret_configured === true ||
    trigger?.linearSecretConfigured === true
  ) {
    return true;
  }
  const status = secretStatus(trigger) as any;
  const source = safeString(
    status.source || status.secret_source || status.secretSource
  ).toLowerCase();
  if (
    status.provider_owned === true ||
    status.providerOwned === true ||
    source === "provider" ||
    source === LINEAR_PROVIDER
  ) {
    return status.configured === true;
  }
  return false;
}

function notionVerification(trigger: any): {
  status: string;
  tokenAvailable: boolean;
} | null {
  const raw = trigger?.verification_status || trigger?.verificationStatus;
  if (!raw || typeof raw !== "object") return null;
  return {
    status: safeString(raw.status),
    tokenAvailable: raw.token_available === true || raw.tokenAvailable === true,
  };
}

const NOTION_STATUS_LABEL: Record<string, string> = {
  awaiting_token: "Waiting for Notion verification token",
  token_received: "Verification token received — copy it into Notion",
  active: "Verified — receiving signed events",
};

function defaultDataClass(trigger: any) {
  return safeString(trigger?.default_data_class || trigger?.defaultDataClass) || "internal";
}

function defaultRiskTier(trigger: any) {
  return safeString(trigger?.default_risk_tier || trigger?.defaultRiskTier);
}

function formatLabel(raw: string) {
  return String(raw || "")
    .split("_")
    .filter(Boolean)
    .map((part) => part.slice(0, 1).toUpperCase() + part.slice(1))
    .join(" ");
}

function formatTime(ms: unknown) {
  const n = typeof ms === "number" ? ms : Number(ms || 0);
  if (!Number.isFinite(n) || n <= 0) return "Never";
  try {
    return new Date(n).toLocaleString();
  } catch {
    return "Never";
  }
}

function statusBadgeClass(status: string) {
  const normalized = String(status || "").toLowerCase();
  if (normalized === "accepted" || normalized === "enabled") return "tcp-badge-ok";
  if (normalized === "rejected" || normalized === "failed" || normalized === "disabled")
    return "tcp-badge-err";
  if (normalized === "duplicate") return "tcp-badge-warn";
  return "tcp-badge-info";
}

function deliveryCounts(trigger: any) {
  return trigger?.delivery_counts || trigger?.deliveryCounts || {};
}

function triggerStatus(trigger: any) {
  if (!trigger?.enabled) return "disabled";
  if (trigger?.last_accepted_at_ms || trigger?.lastAcceptedAtMs) return "last accepted";
  if (trigger?.last_rejected_at_ms || trigger?.lastRejectedAtMs) return "last rejected";
  return "no recent deliveries";
}

async function copyText(value: string) {
  const text = String(value || "").trim();
  if (!text) return false;
  if (navigator?.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return true;
  }
  return false;
}

function normalizeCreateDraft(draft: any) {
  const provider = safeString(draft.provider) || "generic";
  const scheme = safeString(draft.signatureScheme);
  const payload: Record<string, unknown> = {
    name: safeString(draft.name),
    provider,
    enabled: draft.enabled !== false,
    default_data_class: safeString(draft.defaultDataClass) || "internal",
  };
  if (scheme) payload.signature_scheme = scheme;
  const eventKind = safeString(draft.providerEventKind);
  if (eventKind) payload.provider_event_kind = eventKind;
  const risk = safeString(draft.defaultRiskTier);
  if (risk) payload.default_risk_tier = risk;
  const orgUnit = safeString(draft.owningOrgUnitId);
  if (orgUnit) payload.owning_org_unit_id = orgUnit;
  return payload;
}

function normalizeUpdateDraft(draft: any) {
  return {
    name: safeString(draft.name),
    provider: safeString(draft.provider) || "generic",
    provider_event_kind: safeString(draft.providerEventKind) || null,
    signature_scheme: safeString(draft.signatureScheme) || null,
    enabled: draft.enabled !== false,
    default_data_class: safeString(draft.defaultDataClass) || "internal",
    default_risk_tier: safeString(draft.defaultRiskTier) || null,
  };
}

function previewText(value: unknown) {
  if (value === undefined || value === null) return "{}";
  try {
    return JSON.stringify(value, null, 2);
  } catch {
    return String(value);
  }
}

function deliveryReason(delivery: any) {
  return safeString(
    delivery?.rejection_reason_code ||
      delivery?.rejectionReasonCode ||
      delivery?.verification_reason_code ||
      delivery?.verificationReasonCode
  );
}

function isBadSignatureDelivery(delivery: any) {
  return deliveryReason(delivery).toLowerCase() === "bad_signature";
}

function deliveryRunId(delivery: any) {
  return safeString(
    delivery?.queued_run_id ||
      delivery?.queuedRunID ||
      delivery?.woken_run_id ||
      delivery?.wokenRunID
  );
}

function callProviderSecretReplace(
  api: any,
  automationId: string,
  triggerIdValue: string,
  secret: string
) {
  const input = { provider: LINEAR_PROVIDER, secret };
  const method =
    api?.replaceWebhookProviderSecret ||
    api?.replaceWebhookTriggerProviderSecret ||
    api?.importWebhookProviderSecret ||
    api?.importWebhookTriggerSecret ||
    api?.setWebhookProviderSecret;
  if (!method) {
    throw new Error("This engine build does not expose Linear signing secret import yet.");
  }
  return method.call(api, automationId, triggerIdValue, input);
}

export function AutomationWebhookManager({
  client,
  automationId,
  toast,
  onSelectRunId,
  onOpenRunningView,
  onTriggersChanged,
}: {
  client: any;
  automationId: string;
  toast?: (kind: string, message: string) => void;
  onSelectRunId?: (runId: string) => void;
  onOpenRunningView?: () => void;
  onTriggersChanged?: () => void | Promise<void>;
}) {
  const rootRef = useRef<HTMLDivElement | null>(null);
  const queryClient = useQueryClient();
  const [selectedTriggerId, setSelectedTriggerId] = useState("");
  const [revealedSecret, setRevealedSecret] = useState<{
    triggerId: string;
    secret: string;
    label?: string;
    hint?: string;
  } | null>(null);
  const [linearSecretDraft, setLinearSecretDraft] = useState("");
  const [createDraft, setCreateDraft] = useState({
    name: "",
    provider: "generic",
    providerEventKind: "",
    signatureScheme: "",
    defaultDataClass: "internal",
    defaultRiskTier: "",
    owningOrgUnitId: "",
    enabled: true,
  });
  const [editDraft, setEditDraft] = useState({
    name: "",
    provider: "generic",
    providerEventKind: "",
    signatureScheme: "",
    defaultDataClass: "internal",
    defaultRiskTier: "",
    enabled: true,
  });

  const queryKey = ["automations", "webhook-triggers", automationId];
  const triggersQuery = useQuery({
    queryKey,
    enabled: !!automationId && !!client?.automationsV2?.listWebhookTriggers,
    queryFn: () => client.automationsV2.listWebhookTriggers(automationId),
    refetchInterval: 30000,
  });
  const triggers = Array.isArray((triggersQuery.data as any)?.triggers)
    ? ((triggersQuery.data as any).triggers as any[])
    : [];
  const selectedTrigger = useMemo(
    () =>
      triggers.find((trigger) => triggerId(trigger) === selectedTriggerId) || triggers[0] || null,
    [selectedTriggerId, triggers]
  );
  const effectiveTriggerId = triggerId(selectedTrigger);
  const deliveriesQuery = useQuery({
    queryKey: ["automations", "webhook-deliveries", automationId, effectiveTriggerId],
    enabled:
      !!automationId && !!effectiveTriggerId && !!client?.automationsV2?.listWebhookDeliveries,
    queryFn: () => client.automationsV2.listWebhookDeliveries(automationId, effectiveTriggerId, 50),
    refetchInterval: 30000,
  });
  const deliveries = Array.isArray((deliveriesQuery.data as any)?.deliveries)
    ? ((deliveriesQuery.data as any).deliveries as any[])
    : [];

  useEffect(() => {
    if (!triggers.length) {
      if (selectedTriggerId) setSelectedTriggerId("");
      return;
    }
    if (!triggers.some((trigger) => triggerId(trigger) === selectedTriggerId)) {
      setSelectedTriggerId(triggerId(triggers[0]));
    }
  }, [selectedTriggerId, triggers]);

  useEffect(() => {
    if (!selectedTrigger) return;
    setEditDraft({
      name: safeString(selectedTrigger.name || selectedTrigger.provider) || "Webhook trigger",
      provider: safeString(selectedTrigger.provider) || "generic",
      providerEventKind: safeString(
        selectedTrigger.provider_event_kind || selectedTrigger.providerEventKind
      ),
      signatureScheme: signatureScheme(selectedTrigger),
      defaultDataClass: defaultDataClass(selectedTrigger),
      defaultRiskTier: defaultRiskTier(selectedTrigger),
      enabled: selectedTrigger.enabled !== false,
    });
    setLinearSecretDraft("");
  }, [effectiveTriggerId]);

  const invalidate = async () => {
    await queryClient.invalidateQueries({ queryKey });
    if (effectiveTriggerId) {
      await queryClient.invalidateQueries({
        queryKey: ["automations", "webhook-deliveries", automationId, effectiveTriggerId],
      });
    }
    await onTriggersChanged?.();
  };

  const createMutation = useMutation({
    mutationFn: async () =>
      client.automationsV2.createWebhookTrigger(automationId, normalizeCreateDraft(createDraft)),
    onSuccess: async (payload: any) => {
      const nextTrigger = payload?.trigger;
      const nextTriggerId = triggerId(nextTrigger);
      const secret = safeString(payload?.new_secret || payload?.newSecret);
      const nextIsLinear = isLinearProvider(nextTrigger?.provider || createDraft.provider);
      if (nextTriggerId) setSelectedTriggerId(nextTriggerId);
      if (secret && !nextIsLinear) setRevealedSecret({ triggerId: nextTriggerId, secret });
      setCreateDraft({
        name: "",
        provider: "generic",
        providerEventKind: "",
        signatureScheme: "",
        defaultDataClass: "internal",
        defaultRiskTier: "",
        owningOrgUnitId: "",
        enabled: true,
      });
      toast?.(
        "ok",
        nextIsLinear
          ? "Linear webhook trigger created. Import the Linear signing secret before sending live deliveries."
          : "Webhook trigger created."
      );
      await invalidate();
    },
    onError: (error) => toast?.("err", error instanceof Error ? error.message : String(error)),
  });

  const replaceLinearSecretMutation = useMutation({
    mutationFn: async () =>
      callProviderSecretReplace(
        client?.automationsV2,
        automationId,
        effectiveTriggerId,
        linearSecretDraft
      ),
    onSuccess: async () => {
      setLinearSecretDraft("");
      toast?.("ok", "Linear signing secret imported.");
      await invalidate();
    },
    onError: (error) => toast?.("err", error instanceof Error ? error.message : String(error)),
  });

  const updateMutation = useMutation({
    mutationFn: async () =>
      client.automationsV2.updateWebhookTrigger(
        automationId,
        effectiveTriggerId,
        normalizeUpdateDraft(editDraft)
      ),
    onSuccess: async () => {
      toast?.("ok", "Webhook trigger updated.");
      await invalidate();
    },
    onError: (error) => toast?.("err", error instanceof Error ? error.message : String(error)),
  });

  const rotateMutation = useMutation({
    mutationFn: async () =>
      client.automationsV2.rotateWebhookSecret(automationId, effectiveTriggerId),
    onSuccess: async (payload: any) => {
      const secret = safeString(payload?.new_secret || payload?.newSecret);
      if (secret) setRevealedSecret({ triggerId: effectiveTriggerId, secret });
      toast?.("ok", "Webhook secret rotated.");
      await invalidate();
    },
    onError: (error) => toast?.("err", error instanceof Error ? error.message : String(error)),
  });

  const revealTokenMutation = useMutation({
    mutationFn: async () =>
      client.automationsV2.revealWebhookVerificationToken(automationId, effectiveTriggerId),
    onSuccess: async (payload: any) => {
      const token = safeString(payload?.verification_token || payload?.verificationToken);
      if (token) {
        setRevealedSecret({
          triggerId: effectiveTriggerId,
          secret: token,
          label: "Verification token",
          hint: "Paste this token back into Notion to verify the subscription. It is shown once.",
        });
      }
      toast?.("ok", "Verification token revealed.");
      await invalidate();
    },
    onError: (error) => toast?.("err", error instanceof Error ? error.message : String(error)),
  });

  const disableMutation = useMutation({
    mutationFn: async () =>
      client.automationsV2.disableWebhookTrigger(automationId, effectiveTriggerId),
    onSuccess: async () => {
      toast?.("ok", "Webhook trigger disabled.");
      await invalidate();
    },
    onError: (error) => toast?.("err", error instanceof Error ? error.message : String(error)),
  });

  const deleteMutation = useMutation({
    mutationFn: async () =>
      client.automationsV2.deleteWebhookTrigger(automationId, effectiveTriggerId),
    onSuccess: async () => {
      toast?.("ok", "Webhook trigger deleted.");
      setSelectedTriggerId("");
      setRevealedSecret(null);
      await invalidate();
    },
    onError: (error) => toast?.("err", error instanceof Error ? error.message : String(error)),
  });

  const copyCallback = async (trigger: any) => {
    if (await copyText(callbackUrl(trigger))) toast?.("ok", "Callback URL copied.");
  };

  const openQueuedRun = (runId: string) => {
    const id = safeString(runId);
    if (!id) return;
    onSelectRunId?.(id);
    onOpenRunningView?.();
  };

  const setCreateProvider = (provider: string) => {
    setCreateDraft((draft) => ({
      ...draft,
      provider,
      signatureScheme: recommendedSignatureScheme(provider, draft.signatureScheme),
    }));
  };

  const setEditProvider = (provider: string) => {
    setEditDraft((draft) => ({
      ...draft,
      provider,
      signatureScheme: recommendedSignatureScheme(provider, draft.signatureScheme),
    }));
  };

  const createProviderIsLinear = isLinearProvider(createDraft.provider);
  const createSignatureIsUnsigned = createDraft.signatureScheme === UNSIGNED_DEV_MODE_SCHEME;
  const selectedProviderIsLinear =
    !!selectedTrigger && triggerProvider(selectedTrigger) === LINEAR_PROVIDER;
  const selectedSignature = signatureScheme(selectedTrigger);
  const selectedLinearSecretConfigured =
    selectedProviderIsLinear && linearSecretConfigured(selectedTrigger);
  const latestLinearBadSignature = selectedProviderIsLinear
    ? deliveries.find(isBadSignatureDelivery)
    : null;

  return (
    <div ref={rootRef} className="grid gap-4">
      {revealedSecret ? (
        <div className="rounded-lg border border-amber-400/40 bg-amber-400/10 p-3">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div>
              <div className="text-sm font-semibold text-amber-100">
                {revealedSecret.label || "New secret"}
              </div>
              <div className="mt-1 text-xs text-amber-200/80">
                {revealedSecret.hint ||
                  "This secret is shown once. Store it before closing or rotating again."}
              </div>
            </div>
            <button
              type="button"
              className="tcp-btn h-8 px-3 text-xs"
              onClick={() => setRevealedSecret(null)}
            >
              <Icon name="check" />
              Saved
            </button>
          </div>
          <div className="mt-3 flex min-w-0 items-center gap-2 rounded-md border border-amber-400/30 bg-black/20 p-2">
            <code className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-xs text-amber-100">
              {revealedSecret.secret}
            </code>
            <button
              type="button"
              aria-label="Copy revealed webhook secret"
              className="tcp-btn h-8 w-8 px-0"
              onClick={() =>
                void copyText(revealedSecret.secret).then(
                  (ok) => ok && toast?.("ok", "Secret copied.")
                )
              }
            >
              <Icon name="copy" />
            </button>
          </div>
        </div>
      ) : null}

      <div className="grid gap-3 rounded-lg border border-slate-800/70 bg-slate-950/25 p-3">
        <div className="flex flex-wrap items-center justify-between gap-2">
          <div>
            <div className="text-sm font-semibold text-slate-100">Webhook triggers</div>
            <div className="text-xs text-slate-500">
              {triggers.length} configured for this automation
            </div>
          </div>
          <button
            type="button"
            className="tcp-btn h-8 px-3 text-xs"
            onClick={() => void invalidate()}
            disabled={triggersQuery.isFetching}
          >
            <Icon name="refresh-cw" />
            Refresh
          </button>
        </div>

        <div className="grid gap-2 md:grid-cols-2">
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Name</label>
            <input
              className="tcp-input"
              value={createDraft.name}
              onInput={(event) =>
                setCreateDraft((draft) => ({
                  ...draft,
                  name: (event.target as HTMLInputElement).value,
                }))
              }
              placeholder="GitHub issues"
            />
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Provider</label>
            <input
              className="tcp-input"
              list="tcp-webhook-providers"
              value={createDraft.provider}
              onInput={(event) => setCreateProvider((event.target as HTMLInputElement).value)}
              placeholder="generic"
            />
            <datalist id="tcp-webhook-providers">
              <option value="generic"></option>
              <option value="github"></option>
              <option value="gitlab"></option>
              <option value="linear"></option>
              <option value="notion"></option>
              <option value="stripe"></option>
            </datalist>
            {safeString(createDraft.provider).toLowerCase().startsWith("notion") ? (
              <div className="tcp-text-caption text-sky-300/80">
                Notion signs events with a verification token you'll copy from Tandem back into
                Notion after creating the trigger.
              </div>
            ) : null}
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Event kind</label>
            <input
              className="tcp-input"
              value={createDraft.providerEventKind}
              onInput={(event) =>
                setCreateDraft((draft) => ({
                  ...draft,
                  providerEventKind: (event.target as HTMLInputElement).value,
                }))
              }
              placeholder="issues.opened"
            />
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Signature scheme</label>
            <select
              className="tcp-input"
              value={createDraft.signatureScheme}
              onChange={(event) =>
                setCreateDraft((draft) => ({
                  ...draft,
                  signatureScheme: (event.target as HTMLSelectElement).value,
                }))
              }
            >
              {SIGNATURE_SCHEME_OPTIONS.map((option) => (
                <option key={option.value || "default"} value={option.value}>
                  {option.label}
                </option>
              ))}
            </select>
            {createProviderIsLinear && createDraft.signatureScheme !== LINEAR_SIGNATURE_SCHEME ? (
              <div className="tcp-text-caption text-amber-300/90">
                Linear production webhooks should use <code>{LINEAR_SIGNATURE_SCHEME}</code>.
              </div>
            ) : null}
            {createSignatureIsUnsigned ? (
              <div className="tcp-text-caption text-red-300/90">
                Unsigned dev mode is only for local tests. Do not use it with public HTTPS URLs.
              </div>
            ) : null}
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Org unit</label>
            <input
              className="tcp-input"
              value={createDraft.owningOrgUnitId}
              onInput={(event) =>
                setCreateDraft((draft) => ({
                  ...draft,
                  owningOrgUnitId: (event.target as HTMLInputElement).value,
                }))
              }
              placeholder="department-id"
            />
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Data class</label>
            <select
              className="tcp-input"
              value={createDraft.defaultDataClass}
              onChange={(event) =>
                setCreateDraft((draft) => ({
                  ...draft,
                  defaultDataClass: (event.target as HTMLSelectElement).value,
                }))
              }
            >
              {DATA_CLASS_OPTIONS.map((option) => (
                <option key={option} value={option}>
                  {formatLabel(option)}
                </option>
              ))}
            </select>
          </div>
          <div className="grid gap-1">
            <label className="text-xs text-slate-400">Risk tier</label>
            <select
              className="tcp-input"
              value={createDraft.defaultRiskTier}
              onChange={(event) =>
                setCreateDraft((draft) => ({
                  ...draft,
                  defaultRiskTier: (event.target as HTMLSelectElement).value,
                }))
              }
            >
              {RISK_OPTIONS.map((option) => (
                <option key={option || "none"} value={option}>
                  {option ? formatLabel(option) : "None"}
                </option>
              ))}
            </select>
          </div>
        </div>
        {createProviderIsLinear ? (
          <div className="grid gap-2 rounded-md border border-sky-500/30 bg-sky-500/5 p-3 text-xs text-slate-300">
            <div className="font-semibold text-sky-200">Linear setup checklist</div>
            <div className="grid gap-1 text-slate-400">
              <div>Linear Settings -&gt; API -&gt; Webhooks</div>
              <div>Data change events: Issues</div>
              <div>Team selection: desired teams, or all public teams</div>
              <div>
                Project filtering happens in the Tandem automation guard, not in the Linear webhook.
              </div>
            </div>
          </div>
        ) : null}
        <div className="flex flex-wrap items-center justify-between gap-2">
          <label className="flex items-center gap-2 text-xs text-slate-300">
            <input
              type="checkbox"
              checked={createDraft.enabled}
              onChange={(event) =>
                setCreateDraft((draft) => ({
                  ...draft,
                  enabled: (event.target as HTMLInputElement).checked,
                }))
              }
            />
            Enabled on create
          </label>
          <button
            type="button"
            className="tcp-btn-primary h-9 px-3 text-sm"
            disabled={createMutation.isPending || !safeString(createDraft.provider)}
            onClick={() => createMutation.mutate()}
          >
            <Icon name="plus" />
            {createMutation.isPending ? "Creating..." : "Create trigger"}
          </button>
        </div>
      </div>

      {triggersQuery.isLoading ? (
        <div className="text-xs text-slate-500">Loading webhook triggers...</div>
      ) : triggers.length ? (
        <div className="grid gap-3 lg:grid-cols-[minmax(220px,320px)_1fr]">
          <div className="grid content-start gap-2">
            {triggers.map((trigger) => {
              const id = triggerId(trigger);
              const selected = id === effectiveTriggerId;
              const counts = deliveryCounts(trigger);
              return (
                <button
                  key={id}
                  type="button"
                  className={`tcp-list-item text-left ${selected ? "border-amber-400/60 bg-amber-400/10" : ""}`}
                  onClick={() => setSelectedTriggerId(id)}
                >
                  <div className="flex items-start justify-between gap-2">
                    <div className="min-w-0">
                      <div className="truncate text-sm font-semibold text-slate-100">
                        {safeString(trigger.name) || safeString(trigger.provider) || id}
                      </div>
                      <div className="mt-1 truncate text-xs text-slate-500">
                        {safeString(trigger.provider)} ·{" "}
                        {safeString(trigger.provider_event_kind || trigger.providerEventKind) ||
                          "any event"}
                      </div>
                      {triggerProvider(trigger) === LINEAR_PROVIDER ? (
                        <div className="mt-2 flex flex-wrap gap-2">
                          <span
                            className={`tcp-badge ${linearSecretConfigured(trigger) ? "tcp-badge-ok" : "tcp-badge-warn"}`}
                          >
                            {linearSecretConfigured(trigger)
                              ? "Linear secret configured"
                              : "Linear secret missing"}
                          </span>
                          <span
                            className={`tcp-badge ${signatureScheme(trigger) === LINEAR_SIGNATURE_SCHEME ? "tcp-badge-ok" : "tcp-badge-warn"}`}
                          >
                            {signatureScheme(trigger) || "default signature"}
                          </span>
                        </div>
                      ) : null}
                    </div>
                    <span
                      className={`tcp-badge ${statusBadgeClass(trigger.enabled ? "enabled" : "disabled")}`}
                    >
                      {trigger.enabled ? "Enabled" : "Disabled"}
                    </span>
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2 tcp-text-caption text-slate-500">
                    <span>{triggerStatus(trigger)}</span>
                    <span>{Number(counts?.total || 0)} deliveries</span>
                  </div>
                </button>
              );
            })}
          </div>

          {selectedTrigger ? (
            <div className="grid gap-3 rounded-lg border border-slate-800/70 bg-slate-950/25 p-3">
              <div className="flex flex-wrap items-start justify-between gap-2">
                <div className="min-w-0">
                  <div className="text-sm font-semibold text-slate-100">
                    {safeString(selectedTrigger.name) || "Webhook trigger"}
                  </div>
                  <div className="mt-1 truncate text-xs text-slate-500">{effectiveTriggerId}</div>
                </div>
                <span
                  className={`tcp-badge ${statusBadgeClass(selectedTrigger.enabled ? "enabled" : "disabled")}`}
                >
                  {selectedTrigger.enabled ? "Enabled" : "Disabled"}
                </span>
              </div>

              <div className="grid gap-2 rounded-md border border-slate-800/70 bg-black/20 p-2">
                <div className="text-xs uppercase tracking-[0.16em] text-slate-500">Callback</div>
                <div className="flex min-w-0 items-center gap-2">
                  <code className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-xs text-slate-300">
                    {callbackUrl(selectedTrigger) || "No callback URL"}
                  </code>
                  <button
                    type="button"
                    aria-label="Copy callback URL"
                    className="tcp-btn h-8 w-8 px-0"
                    onClick={() => void copyCallback(selectedTrigger)}
                  >
                    <Icon name="copy" />
                  </button>
                </div>
              </div>

              {(() => {
                const verification = notionVerification(selectedTrigger);
                if (!verification) return null;
                const label =
                  NOTION_STATUS_LABEL[verification.status] || formatLabel(verification.status);
                return (
                  <div className="grid gap-2 rounded-md border border-sky-500/30 bg-sky-500/5 p-2">
                    <div className="flex items-center justify-between gap-2">
                      <div className="text-xs uppercase tracking-[0.16em] text-sky-300">
                        Notion verification
                      </div>
                      <span
                        className={`tcp-badge ${verification.status === "active" ? "tcp-badge-ok" : "tcp-badge-info"}`}
                      >
                        {label}
                      </span>
                    </div>
                    <div className="text-xs text-slate-400">
                      Paste the callback URL into your Notion connection's Webhooks tab. When Notion
                      sends its verification token, reveal it here (once) and paste it back into
                      Notion to activate the subscription.
                    </div>
                    <button
                      type="button"
                      className="tcp-btn h-8 justify-self-start px-3 text-xs"
                      disabled={!verification.tokenAvailable || revealTokenMutation.isPending}
                      onClick={() => revealTokenMutation.mutate()}
                    >
                      <Icon name="key-round" />
                      {verification.tokenAvailable
                        ? "Reveal verification token"
                        : "No token to reveal"}
                    </button>
                  </div>
                );
              })()}

              {selectedProviderIsLinear ? (
                <div className="grid gap-3 rounded-md border border-sky-500/30 bg-sky-500/5 p-3">
                  <div className="flex flex-wrap items-center justify-between gap-2">
                    <div>
                      <div className="text-xs uppercase tracking-[0.16em] text-sky-300">
                        Linear setup
                      </div>
                      <div className="mt-1 text-xs text-slate-400">
                        Use this callback URL in Linear, then paste Linear's signing secret here.
                      </div>
                    </div>
                    <span
                      className={`tcp-badge ${selectedLinearSecretConfigured ? "tcp-badge-ok" : "tcp-badge-warn"}`}
                    >
                      {selectedLinearSecretConfigured
                        ? "Signing secret configured"
                        : "Signing secret not configured"}
                    </span>
                  </div>
                  <div className="grid gap-2 rounded-md border border-slate-800/70 bg-black/20 p-2">
                    <div className="text-xs text-slate-400">Linear URL field</div>
                    <div className="flex min-w-0 items-center gap-2">
                      <code className="min-w-0 flex-1 overflow-hidden text-ellipsis whitespace-nowrap text-xs text-slate-300">
                        {callbackUrl(selectedTrigger) || "No callback URL"}
                      </code>
                      <button
                        type="button"
                        aria-label="Copy callback URL"
                        className="tcp-btn h-8 w-8 px-0"
                        onClick={() => void copyCallback(selectedTrigger)}
                      >
                        <Icon name="copy" />
                      </button>
                    </div>
                  </div>
                  <div className="grid gap-1 text-xs text-slate-400">
                    <div>Linear Settings -&gt; API -&gt; Webhooks</div>
                    <div>URL: the Tandem callback URL above</div>
                    <div>Data change events: Issues</div>
                    <div>Team selection: desired teams, or all public teams</div>
                    <div>Keep project filtering in the Tandem automation guard.</div>
                  </div>
                  {selectedSignature !== LINEAR_SIGNATURE_SCHEME ? (
                    <div className="rounded-md border border-amber-400/30 bg-amber-400/10 p-2 text-xs text-amber-100">
                      This trigger is using <code>{selectedSignature || "provider default"}</code>.
                      Linear production deliveries should use <code>{LINEAR_SIGNATURE_SCHEME}</code>
                      .
                    </div>
                  ) : null}
                  {selectedSignature === UNSIGNED_DEV_MODE_SCHEME ? (
                    <div className="rounded-md border border-red-400/30 bg-red-400/10 p-2 text-xs text-red-100">
                      Unsigned dev mode is not safe for public HTTPS URLs.
                    </div>
                  ) : null}
                  <div className="grid gap-2 md:grid-cols-[1fr_auto]">
                    <input
                      className="tcp-input"
                      type="password"
                      value={linearSecretDraft}
                      onInput={(event) =>
                        setLinearSecretDraft((event.target as HTMLInputElement).value)
                      }
                      placeholder="Paste Linear signing secret"
                      autoComplete="off"
                    />
                    <button
                      type="button"
                      className="tcp-btn h-9 px-3 text-sm"
                      disabled={
                        !safeString(linearSecretDraft) ||
                        !effectiveTriggerId ||
                        replaceLinearSecretMutation.isPending
                      }
                      onClick={() => replaceLinearSecretMutation.mutate()}
                    >
                      <Icon name="key-round" />
                      {replaceLinearSecretMutation.isPending ? "Importing..." : "Import secret"}
                    </button>
                  </div>
                  {latestLinearBadSignature ? (
                    <div className="rounded-md border border-amber-400/30 bg-amber-400/10 p-2 text-xs text-amber-100">
                      Latest signature failure at{" "}
                      {formatTime(
                        latestLinearBadSignature.received_at_ms ||
                          latestLinearBadSignature.receivedAtMs
                      )}
                      . Re-import the Linear signing secret if deliveries now show{" "}
                      <code>bad_signature</code>.
                    </div>
                  ) : null}
                </div>
              ) : null}

              <div className="grid gap-2 md:grid-cols-2">
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Name</label>
                  <input
                    className="tcp-input"
                    value={editDraft.name}
                    onInput={(event) =>
                      setEditDraft((draft) => ({
                        ...draft,
                        name: (event.target as HTMLInputElement).value,
                      }))
                    }
                  />
                </div>
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Provider</label>
                  <input
                    className="tcp-input"
                    value={editDraft.provider}
                    onInput={(event) => setEditProvider((event.target as HTMLInputElement).value)}
                  />
                </div>
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Event kind</label>
                  <input
                    className="tcp-input"
                    value={editDraft.providerEventKind}
                    onInput={(event) =>
                      setEditDraft((draft) => ({
                        ...draft,
                        providerEventKind: (event.target as HTMLInputElement).value,
                      }))
                    }
                  />
                </div>
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Signature scheme</label>
                  <select
                    className="tcp-input"
                    value={editDraft.signatureScheme}
                    onChange={(event) =>
                      setEditDraft((draft) => ({
                        ...draft,
                        signatureScheme: (event.target as HTMLSelectElement).value,
                      }))
                    }
                  >
                    {SIGNATURE_SCHEME_OPTIONS.map((option) => (
                      <option key={option.value || "default"} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Data class</label>
                  <select
                    className="tcp-input"
                    value={editDraft.defaultDataClass}
                    onChange={(event) =>
                      setEditDraft((draft) => ({
                        ...draft,
                        defaultDataClass: (event.target as HTMLSelectElement).value,
                      }))
                    }
                  >
                    {DATA_CLASS_OPTIONS.map((option) => (
                      <option key={option} value={option}>
                        {formatLabel(option)}
                      </option>
                    ))}
                  </select>
                </div>
                <div className="grid gap-1">
                  <label className="text-xs text-slate-400">Risk tier</label>
                  <select
                    className="tcp-input"
                    value={editDraft.defaultRiskTier}
                    onChange={(event) =>
                      setEditDraft((draft) => ({
                        ...draft,
                        defaultRiskTier: (event.target as HTMLSelectElement).value,
                      }))
                    }
                  >
                    {RISK_OPTIONS.map((option) => (
                      <option key={option || "none"} value={option}>
                        {option ? formatLabel(option) : "None"}
                      </option>
                    ))}
                  </select>
                </div>
                <label className="flex items-center gap-2 self-end pb-2 text-xs text-slate-300">
                  <input
                    type="checkbox"
                    checked={editDraft.enabled}
                    onChange={(event) =>
                      setEditDraft((draft) => ({
                        ...draft,
                        enabled: (event.target as HTMLInputElement).checked,
                      }))
                    }
                  />
                  Enabled
                </label>
              </div>

              <div className="flex flex-wrap gap-2">
                <button
                  type="button"
                  className="tcp-btn h-9 px-3 text-sm"
                  disabled={updateMutation.isPending || !effectiveTriggerId}
                  onClick={() => updateMutation.mutate()}
                >
                  <Icon name="save" />
                  {updateMutation.isPending ? "Saving..." : "Save trigger"}
                </button>
                {notionVerification(selectedTrigger) || selectedProviderIsLinear ? null : (
                  <button
                    type="button"
                    className="tcp-btn h-9 px-3 text-sm"
                    disabled={rotateMutation.isPending || !effectiveTriggerId}
                    onClick={() => rotateMutation.mutate()}
                  >
                    <Icon name="rotate-cw" />
                    {rotateMutation.isPending ? "Rotating..." : "Rotate secret"}
                  </button>
                )}
                <button
                  type="button"
                  className="tcp-btn h-9 px-3 text-sm"
                  disabled={
                    disableMutation.isPending ||
                    !effectiveTriggerId ||
                    selectedTrigger.enabled === false
                  }
                  onClick={() => disableMutation.mutate()}
                >
                  <Icon name="pause-circle" />
                  {disableMutation.isPending ? "Disabling..." : "Disable"}
                </button>
                <button
                  type="button"
                  className="tcp-btn h-9 px-3 text-sm text-red-200"
                  disabled={deleteMutation.isPending || !effectiveTriggerId}
                  onClick={() => {
                    if (window.confirm("Delete this webhook trigger?")) deleteMutation.mutate();
                  }}
                >
                  <Icon name="trash-2" />
                  {deleteMutation.isPending ? "Deleting..." : "Delete"}
                </button>
              </div>

              <div className="grid gap-2">
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div>
                    <div className="text-sm font-semibold text-slate-100">Recent deliveries</div>
                    <div className="text-xs text-slate-500">
                      {deliveries.length} visible for this trigger
                    </div>
                  </div>
                  <button
                    type="button"
                    className="tcp-btn h-8 px-3 text-xs"
                    onClick={() => void deliveriesQuery.refetch()}
                    disabled={deliveriesQuery.isFetching}
                  >
                    <Icon name="refresh-cw" />
                    Refresh
                  </button>
                </div>
                {deliveries.length ? (
                  <div className="grid gap-2">
                    {deliveries.map((delivery) => {
                      const id = deliveryId(delivery);
                      const status = safeString(delivery.status) || "received";
                      const runId = deliveryRunId(delivery);
                      const providerEventId = safeString(
                        delivery.provider_event_id || delivery.providerEventID
                      );
                      const reason = deliveryReason(delivery);
                      return (
                        <details
                          key={id}
                          className="rounded-lg border border-slate-800/70 bg-black/20 p-3"
                        >
                          <summary className="cursor-pointer list-none">
                            <div className="flex flex-wrap items-center justify-between gap-2">
                              <div className="min-w-0">
                                <div className="truncate text-sm font-medium text-slate-200">
                                  {id || "delivery"}
                                </div>
                                <div className="mt-1 text-xs text-slate-500">
                                  {formatTime(delivery.received_at_ms || delivery.receivedAtMs)}
                                </div>
                              </div>
                              <span className={`tcp-badge ${statusBadgeClass(status)}`}>
                                {formatLabel(status)}
                              </span>
                            </div>
                          </summary>
                          <div className="mt-3 grid gap-2 text-xs text-slate-400">
                            <div className="grid gap-1 md:grid-cols-2">
                              <div>
                                event: <code>{providerEventId || "n/a"}</code>
                              </div>
                              <div>
                                digest:{" "}
                                <code>
                                  {safeString(delivery.body_digest || delivery.bodyDigest) || "n/a"}
                                </code>
                              </div>
                              <div>
                                reason: <code>{reason || "n/a"}</code>
                              </div>
                              {selectedProviderIsLinear ? (
                                <div>
                                  Linear delivery: <code>{providerEventId || id || "n/a"}</code>
                                </div>
                              ) : null}
                              <div>
                                run:{" "}
                                {runId ? (
                                  <button
                                    type="button"
                                    className="text-amber-200 underline decoration-amber-400/40 underline-offset-2"
                                    onClick={() => openQueuedRun(runId)}
                                  >
                                    {runId}
                                  </button>
                                ) : (
                                  <span>n/a</span>
                                )}
                              </div>
                            </div>
                            <pre className="max-h-56 overflow-auto whitespace-pre-wrap rounded-md border border-slate-800/70 bg-slate-950/70 p-2 tcp-text-caption leading-5 text-slate-300">
                              {previewText(delivery.sanitized_preview || delivery.sanitizedPreview)}
                            </pre>
                          </div>
                        </details>
                      );
                    })}
                  </div>
                ) : (
                  <div className="rounded-md border border-slate-800/70 bg-black/20 p-3 text-xs text-slate-500">
                    No deliveries have been recorded for this trigger.
                  </div>
                )}
              </div>
            </div>
          ) : null}
        </div>
      ) : (
        <div className="rounded-md border border-slate-800/70 bg-black/20 p-3 text-xs text-slate-500">
          No webhook triggers are configured for this automation.
        </div>
      )}
    </div>
  );
}
