import { useMemo, useState } from "react";
import { useEngineStream } from "../features/stream/useEngineStream";
import { PageCard, EmptyState, formatJson } from "./ui";
import type { AppPageProps } from "./pageTypes";

function eventTypeOf(data: any) {
  return data?.type || data?.event || "event";
}

export function FeedPage({ client, toast, navigate }: AppPageProps) {
  const [events, setEvents] = useState<Array<{ at: number; data: any }>>([]);
  const [filter, setFilter] = useState("");
  const [group, setGroup] = useState("all");

  useEngineStream(
    "/api/engine/global/event",
    (event) => {
      try {
        const data = JSON.parse(event.data);
        setEvents((prev) => [...prev.slice(-299), { at: Date.now(), data }]);
      } catch {
        // ignore malformed events
      }
    },
    {
      enabled: true,
      onError: () => toast("err", "Live feed disconnected."),
    }
  );

  const groupedTypes = useMemo(() => {
    const counts = new Map<string, number>();
    for (const item of events) {
      const key = eventTypeOf(item.data);
      counts.set(key, (counts.get(key) || 0) + 1);
    }
    return [...counts.entries()].sort((a, b) => b[1] - a[1]);
  }, [events]);

  const filtered = useMemo(() => {
    const term = filter.trim().toLowerCase();
    return events
      .filter((item) => {
        const type = eventTypeOf(item.data);
        if (group !== "all" && type !== group) return false;
        if (!term) return true;
        return `${type} ${JSON.stringify(item.data || {})}`.toLowerCase().includes(term);
      })
      .slice(-240)
      .reverse();
  }, [events, filter, group]);

  async function installFromPath(path: string) {
    try {
      const payload = await client.packs.install({
        path,
        source: { kind: "control_panel_feed", event: "pack.detected" },
      });
      toast("ok", `Installed ${payload?.installed?.name || "pack"}`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }

  async function installFromAttachment(evt: any) {
    try {
      const payload = await client.packs.installFromAttachment({
        attachment_id: String(evt?.properties?.attachment_id || evt?.attachment_id || ""),
        path: String(evt?.properties?.path || evt?.path || ""),
        connector: String(evt?.properties?.connector || evt?.connector || "") || undefined,
        channel_id: String(evt?.properties?.channel_id || evt?.channel_id || "") || undefined,
        sender_id: String(evt?.properties?.sender_id || evt?.sender_id || "") || undefined,
      });
      toast("ok", `Installed ${payload?.installed?.name || "pack"}`);
    } catch (error) {
      toast("err", error instanceof Error ? error.message : String(error));
    }
  }

  return (
    <div className="grid gap-4">
      <PageCard title="Global Live Feed" subtitle="Streaming events across engine and packs">
        <div className="mb-3 flex flex-wrap items-center gap-2">
          <input
            className="tcp-input min-w-[220px]"
            value={filter}
            onInput={(e) => setFilter((e.target as HTMLInputElement).value)}
            placeholder="Filter by type or payload"
          />
          <button className="tcp-btn" onClick={() => setEvents([])}>
            <i data-lucide="trash-2"></i>
            Clear
          </button>
          <div className="flex flex-wrap gap-1">
            <button
              className={`tcp-btn h-7 px-2 text-xs ${group === "all" ? "border-amber-400/60" : ""}`}
              onClick={() => setGroup("all")}
            >
              <i data-lucide="list"></i>
              All
            </button>
            {groupedTypes.slice(0, 8).map(([type, count]) => (
              <button
                key={type}
                className={`tcp-btn h-7 px-2 text-xs ${group === type ? "border-amber-400/60" : ""}`}
                onClick={() => setGroup(type)}
              >
                <i data-lucide="activity"></i>
                {type} ({count})
              </button>
            ))}
          </div>
        </div>

        <div className="grid max-h-[66vh] gap-2 overflow-auto rounded-xl border border-slate-700 bg-black/20 p-2">
          {filtered.length ? (
            filtered.map((x, index) => {
              const type = eventTypeOf(x.data);
              const isPack = String(type).startsWith("pack.");
              const path = String(x.data?.properties?.path || x.data?.path || "");
              const attachmentId = String(
                x.data?.properties?.attachment_id || x.data?.attachment_id || ""
              );
              return (
                <article key={`${x.at}-${index}`} className="tcp-list-item">
                  <div className="flex items-center justify-between gap-2">
                    <strong>{type}</strong>
                    <span className="tcp-badge-info">{new Date(x.at).toLocaleTimeString()}</span>
                  </div>
                  <p className="tcp-subtle mt-1 text-xs">
                    session: {String(x.data?.sessionID || x.data?.sessionId || "n/a")}
                  </p>
                  {isPack ? (
                    <div className="mt-2 flex flex-wrap gap-2">
                      <button
                        className="tcp-btn h-7 px-2 text-xs"
                        onClick={() => navigate("packs")}
                      >
                        <i data-lucide="package"></i>
                        Open Pack Library
                      </button>
                      {path ? (
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => installFromPath(path)}
                        >
                          <i data-lucide="download"></i>
                          Install from Path
                        </button>
                      ) : null}
                      {path && attachmentId ? (
                        <button
                          className="tcp-btn h-7 px-2 text-xs"
                          onClick={() => installFromAttachment(x.data)}
                        >
                          <i data-lucide="paperclip"></i>
                          Install Attachment
                        </button>
                      ) : null}
                    </div>
                  ) : null}
                  <details className="mt-2">
                    <summary className="cursor-pointer text-xs text-slate-400">Payload</summary>
                    <pre className="tcp-code mt-2">{formatJson(x.data)}</pre>
                  </details>
                </article>
              );
            })
          ) : (
            <EmptyState text="No events yet." />
          )}
        </div>
      </PageCard>
    </div>
  );
}
