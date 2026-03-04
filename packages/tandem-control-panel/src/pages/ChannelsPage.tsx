import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
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
      await client.channels.put(channel, payload);
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
