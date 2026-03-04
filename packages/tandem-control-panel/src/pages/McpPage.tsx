import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function McpPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const serversQuery = useQuery({
    queryKey: ["mcp", "servers"],
    queryFn: () => client.mcp.list().catch(() => ({ servers: [] })),
    refetchInterval: 10000,
  });
  const toolsQuery = useQuery({
    queryKey: ["mcp", "tools"],
    queryFn: () => client.mcp.listTools().catch(() => ({ tools: [] })),
    refetchInterval: 15000,
  });

  const actionMutation = useMutation({
    mutationFn: async ({
      name,
      action,
    }: {
      name: string;
      action: "connect" | "disconnect" | "refresh";
    }) => {
      if (action === "connect") return client.mcp.connect(name);
      if (action === "disconnect") return client.mcp.disconnect(name);
      return client.mcp.refresh(name);
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["mcp"] });
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const servers = toArray(serversQuery.data, "servers");
  const tools = toArray(toolsQuery.data, "tools");

  return (
    <div className="grid gap-4 xl:grid-cols-2">
      <PageCard title="MCP Servers" subtitle="Connected tool servers">
        <div className="grid gap-2">
          {servers.length ? (
            servers.map((server: any, index: number) => {
              const name = String(server?.name || server?.id || `server-${index}`);
              return (
                <div key={name} className="tcp-list-item">
                  <div className="mb-1 flex items-center justify-between gap-2">
                    <strong>{name}</strong>
                    <span className={server?.connected ? "tcp-badge-ok" : "tcp-badge-warn"}>
                      {server?.connected ? "connected" : "disconnected"}
                    </span>
                  </div>
                  <div className="mt-2 flex flex-wrap gap-2">
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => actionMutation.mutate({ name, action: "connect" })}
                    >
                      Connect
                    </button>
                    <button
                      className="tcp-btn h-7 px-2 text-xs"
                      onClick={() => actionMutation.mutate({ name, action: "refresh" })}
                    >
                      Refresh
                    </button>
                    <button
                      className="tcp-btn-danger h-7 px-2 text-xs"
                      onClick={() => actionMutation.mutate({ name, action: "disconnect" })}
                    >
                      Disconnect
                    </button>
                  </div>
                </div>
              );
            })
          ) : (
            <EmptyState text="No MCP servers found." />
          )}
        </div>
      </PageCard>

      <PageCard title="Discovered Tools" subtitle="Tools available across MCP servers">
        <div className="grid max-h-[60vh] gap-2 overflow-auto">
          {tools.length ? (
            tools.slice(0, 120).map((tool: any, index: number) => (
              <div key={String(tool?.name || tool?.id || index)} className="tcp-list-item">
                <div className="font-medium">{String(tool?.name || tool?.id || "tool")}</div>
                <div className="tcp-subtle text-xs">
                  {String(tool?.description || tool?.server || "")}
                </div>
              </div>
            ))
          ) : (
            <EmptyState text="No tools discovered yet." />
          )}
        </div>
      </PageCard>
    </div>
  );
}
