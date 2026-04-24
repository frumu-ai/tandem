import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import type { ChannelName } from "@frumu/tandem-client";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

export function ChannelsPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: ["channels", "status"],
    queryFn: () => client.channels.status().catch(() => ({})),
    refetchInterval: 6000,
  });
  const configQuery = useQuery({
    queryKey: ["channels", "config"],
    queryFn: () => client.channels.config().catch(() => ({})),
    refetchInterval: 15000,
  });

  const reconnectMutation = useMutation({
    mutationFn: async (channel: string) => {
      const config = (configQuery.data || {}) as Record<string, any>;
      const payload = config[channel];
      if (!payload) throw new Error(`No config found for ${channel}`);
      await client.channels.put(channel as ChannelName, payload);
    },
    onSuccess: async () => {
      toast("ok", "Channel reconfigured.");
      await queryClient.invalidateQueries({ queryKey: ["channels"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const status = statusQuery.data && typeof statusQuery.data === "object" ? statusQuery.data : {};
  const rows = Object.entries(status);

  return (
    <div className="grid gap-4">
      <PageCard
        title="Chat automation drafts"
        subtitle="How Tandem turns channel messages into bounded automations"
      >
        <div className="grid gap-3 text-sm">
          <p className="tcp-subtle">
            When someone asks Tandem to create an automation from Discord, Telegram, Slack, or a
            direct chat, Tandem keeps the draft in that same conversation instead of opening the
            workflow editor.
          </p>
          <div className="grid gap-2 md:grid-cols-3">
            <div className="tcp-list-item">
              <strong>Questions</strong>
              <div className="tcp-subtle mt-1 text-xs">
                Tandem captures the next non-command reply from the same person in the same chat
                scope.
              </div>
            </div>
            <div className="tcp-list-item">
              <strong>Confirmation</strong>
              <div className="tcp-subtle mt-1 text-xs">
                The draft is only created after a plain text confirm. Reply cancel to stop.
              </div>
            </div>
            <div className="tcp-list-item">
              <strong>Bounds</strong>
              <div className="tcp-subtle mt-1 text-xs">
                The created automation records the source channel, sender, scope, and allowed tool
                policy.
              </div>
            </div>
          </div>
        </div>
      </PageCard>
      <PageCard title="Channels" subtitle="Connector health and quick reconnect">
        <div className="grid gap-2">
          {rows.length ? (
            rows.map(([name, row]: [string, any]) => (
              <div key={name} className="tcp-list-item">
                <div className="mb-1 flex items-center justify-between gap-2">
                  <strong>{name}</strong>
                  <span className={row?.connected ? "tcp-badge-ok" : "tcp-badge-warn"}>
                    {row?.connected ? "connected" : "disconnected"}
                  </span>
                </div>
                <div className="tcp-subtle text-xs">
                  {String(row?.last_error || row?.error || "") || "No recent errors."}
                </div>
                <div className="mt-2">
                  <button
                    className="tcp-btn h-7 px-2 text-xs"
                    onClick={() => reconnectMutation.mutate(name)}
                  >
                    <i data-lucide="refresh-cw"></i>
                    Reconnect
                  </button>
                </div>
              </div>
            ))
          ) : (
            <EmptyState text="No channels configured." />
          )}
        </div>
      </PageCard>
    </div>
  );
}
