import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";
import { Badge } from "../ui/index";
import { Icon } from "../ui/Icon";
import {
  ingressModeLabel,
  normalizeSlackConnections,
  normalizeSlackSenders,
  parseOrgUnitsInput,
  senderTone,
  verifyResultsByChannel,
} from "./channelConnectionsModel.mjs";

type SenderRow = ReturnType<typeof normalizeSlackSenders>[number];

/**
 * Channel Connections (TAN-766): manage the per-channel Slack connections
 * introduced in TAN-763 — installation identity, secret presence, tenant and
 * department bindings — verify live bot bindings, and map recently seen
 * senders to departments (TAN-765).
 */
export function ChannelConnectionsPage({ client, api, toast, navigate }: AppPageProps) {
  const queryClient = useQueryClient();
  const configQuery = useQuery({
    queryKey: ["channel-connections", "config"],
    queryFn: () => client.channels.config().catch(() => ({})),
    refetchInterval: 15000,
  });
  const statusQuery = useQuery({
    queryKey: ["channel-connections", "status"],
    queryFn: () => client.channels.status().catch(() => ({})),
    refetchInterval: 6000,
  });
  const sendersQuery = useQuery({
    queryKey: ["channel-connections", "slack-senders"],
    queryFn: () => api("/api/engine/channels/slack/senders").catch(() => ({ senders: [] })),
    refetchInterval: 15000,
  });

  const [verifyResults, setVerifyResults] = useState<Map<string, any> | null>(null);
  const verifyMutation = useMutation({
    mutationFn: () =>
      api("/api/engine/channels/slack/verify", {
        method: "POST",
        body: JSON.stringify({}),
        headers: { "content-type": "application/json" },
      }),
    onSuccess: (payload) => {
      setVerifyResults(verifyResultsByChannel(payload));
      toast(
        payload?.ok ? "ok" : "warn",
        payload?.ok
          ? "All Slack connections verified."
          : "Some Slack connections failed verification."
      );
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const [enrollTarget, setEnrollTarget] = useState<SenderRow | null>(null);
  const [enrollUnits, setEnrollUnits] = useState("");
  const [issuedCode, setIssuedCode] = useState<{ principal: string; code: string } | null>(null);
  const enrollMutation = useMutation({
    mutationFn: (input: { principal: string; orgUnits: string[] }) =>
      api("/api/engine/channels/enroll", {
        method: "POST",
        body: JSON.stringify({
          action: "issue",
          channel: "slack",
          user_id: input.principal,
          tier: "approve",
          org_units: input.orgUnits,
        }),
        headers: { "content-type": "application/json" },
      }),
    onSuccess: (payload, input) => {
      const code = String(payload?.pairing_code || "");
      setIssuedCode(code ? { principal: input.principal, code } : null);
      setEnrollTarget(null);
      setEnrollUnits("");
      toast("ok", code ? `Pairing code issued: ${code}` : "Pairing code issued.");
      queryClient.invalidateQueries({ queryKey: ["channel-connections", "slack-senders"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const connections = normalizeSlackConnections(configQuery.data || {});
  const senders = normalizeSlackSenders(sendersQuery.data || {});
  const slackStatus: any =
    statusQuery.data && typeof statusQuery.data === "object"
      ? (statusQuery.data as Record<string, any>).slack
      : null;

  const copyPrincipal = async (principal: string) => {
    try {
      await navigator.clipboard.writeText(principal);
      toast("ok", "Principal copied — paste it as member_id in Enterprise admin.");
    } catch {
      toast("warn", principal);
    }
  };

  return (
    <div className="grid gap-4">
      <PageCard
        title="Slack channel connections"
        subtitle="Per-channel installation identity, tenant and department bindings (TAN-763/764)"
      >
        <div className="mb-3 flex items-center justify-between gap-2">
          <div className="tcp-subtle text-xs">
            {slackStatus
              ? slackStatus.connected
                ? "Slack listener/ingress is up."
                : String(slackStatus.last_error || "Slack is not connected.")
              : "No live Slack status yet."}
          </div>
          <button
            className="tcp-btn h-7 px-2 text-xs"
            disabled={verifyMutation.isPending}
            onClick={() => verifyMutation.mutate()}
            data-testid="verify-slack-connections"
          >
            <Icon name="shield-check" />
            {verifyMutation.isPending ? "Verifying…" : "Verify bindings"}
          </button>
        </div>
        <div className="grid gap-2">
          {connections.length ? (
            connections.map((connection) => {
              const verify = verifyResults?.get(connection.channelId);
              return (
                <div key={connection.channelId} className="tcp-list-item">
                  <div className="mb-1 flex flex-wrap items-center justify-between gap-2">
                    <strong>{connection.channelId}</strong>
                    <div className="flex flex-wrap items-center gap-1">
                      <Badge tone={connection.eventsCapable ? "ok" : "warn"}>
                        {ingressModeLabel(connection)}
                      </Badge>
                      <Badge tone={connection.hasToken ? "ok" : "err"}>
                        {connection.hasToken ? "token set" : "no token"}
                      </Badge>
                      <Badge tone={connection.hasSigningSecret ? "ok" : "warn"}>
                        {connection.hasSigningSecret ? "signing secret set" : "no signing secret"}
                      </Badge>
                      {verify ? (
                        <Badge tone={verify.ok ? "ok" : "err"}>
                          {verify.ok ? "verified" : "verify failed"}
                        </Badge>
                      ) : null}
                    </div>
                  </div>
                  <div className="tcp-subtle text-xs">
                    Workspace {connection.teamId || "—"} · App {connection.appId || "—"} · Tenant{" "}
                    {connection.tenantOrgId
                      ? `${connection.tenantOrgId}/${connection.tenantWorkspaceId}`
                      : "unbound"}
                    {connection.mentionOnly ? " · mention-only" : ""}
                    {connection.notifyApprovals ? " · approval cards on" : " · approval cards off"}
                  </div>
                  <div className="tcp-subtle mt-1 text-xs">
                    Departments:{" "}
                    {connection.orgUnits.length ? connection.orgUnits.join(", ") : "none bound"}
                  </div>
                  {verify && !verify.ok && verify.error ? (
                    <div className="mt-1 text-xs text-red-400">{verify.error}</div>
                  ) : null}
                </div>
              );
            })
          ) : (
            <EmptyState text="No Slack connections configured. Add channels.slack (or channels.slack.connections) in the engine config." />
          )}
        </div>
      </PageCard>

      <PageCard
        title="Recently seen senders"
        subtitle="Map Slack identities to departments without composing principal strings (TAN-765)"
      >
        {issuedCode ? (
          <div className="tcp-list-item mb-2 text-sm" data-testid="issued-pairing-code">
            Pairing code <strong>{issuedCode.code}</strong> issued for{" "}
            <code className="text-xs">{issuedCode.principal}</code>. Hand it to the user to redeem;
            redeeming binds the departments and the approve tier.
          </div>
        ) : null}
        <div className="grid gap-2">
          {senders.length ? (
            senders.map((sender) => (
              <div key={sender.principal} className="tcp-list-item">
                <div className="mb-1 flex flex-wrap items-center justify-between gap-2">
                  <strong>{sender.userId}</strong>
                  <div className="flex flex-wrap items-center gap-1">
                    <Badge tone={senderTone(sender)}>
                      {sender.mapped ? "mapped" : "unmapped"}
                    </Badge>
                    <span className="tcp-subtle text-xs">
                      {sender.acceptedCount} accepted · {sender.deniedCount} denied
                    </span>
                  </div>
                </div>
                <div className="tcp-subtle break-all text-xs">{sender.principal}</div>
                <div className="tcp-subtle mt-1 text-xs">
                  Channels: {sender.channels.length ? sender.channels.join(", ") : "—"} ·
                  Departments: {sender.orgUnits.length ? sender.orgUnits.join(", ") : "none"}
                </div>
                {!sender.mapped && sender.lastDenialReason ? (
                  <div className="mt-1 text-xs text-amber-400">{sender.lastDenialReason}</div>
                ) : null}
                <div className="mt-2 flex flex-wrap items-center gap-2">
                  <button
                    className="tcp-btn h-7 px-2 text-xs"
                    onClick={() => copyPrincipal(sender.principal)}
                  >
                    <Icon name="copy" />
                    Copy principal
                  </button>
                  <button
                    className="tcp-btn h-7 px-2 text-xs"
                    onClick={() => {
                      setEnrollTarget(sender);
                      setEnrollUnits(sender.orgUnits.join(", "));
                    }}
                  >
                    <Icon name="shield-check" />
                    Issue pairing code
                  </button>
                  <button
                    className="tcp-btn h-7 px-2 text-xs"
                    onClick={() => navigate("enterprise-admin")}
                  >
                    <Icon name="shield" />
                    Enterprise admin
                  </button>
                </div>
                {enrollTarget?.principal === sender.principal ? (
                  <div className="mt-2 grid gap-2 md:grid-cols-[1fr_auto]">
                    <input
                      className="tcp-input h-8 text-xs"
                      placeholder="Departments, e.g. department/sales, engineering"
                      value={enrollUnits}
                      onChange={(event) => setEnrollUnits(event.currentTarget.value)}
                      data-testid="enroll-org-units"
                    />
                    <button
                      className="tcp-btn tcp-btn-primary h-8 px-3 text-xs"
                      disabled={enrollMutation.isPending}
                      onClick={() =>
                        enrollMutation.mutate({
                          principal: sender.principal,
                          orgUnits: parseOrgUnitsInput(enrollUnits),
                        })
                      }
                    >
                      {enrollMutation.isPending ? "Issuing…" : "Issue code"}
                    </button>
                  </div>
                ) : null}
              </div>
            ))
          ) : (
            <EmptyState text="No Slack senders observed yet. Senders appear after signed Events ingress accepts or denies a message." />
          )}
        </div>
      </PageCard>
    </div>
  );
}
