import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AnimatePresence, motion } from "motion/react";
import { useCallback, useEffect, useMemo, useState } from "react";
import { PageCard, EmptyState } from "./ui";
import type { AppPageProps } from "./pageTypes";

const CHAT_UPLOAD_DIR = "control-panel";
const CHAT_SESSION_KEY = "tcp.chat.session";

function toArray(input: any, key: string) {
  if (Array.isArray(input)) return input;
  if (Array.isArray(input?.[key])) return input[key];
  return [];
}

export function ChatPage({ client, toast }: AppPageProps) {
  const queryClient = useQueryClient();
  const [selectedSessionId, setSelectedSessionId] = useState("");
  const [prompt, setPrompt] = useState("");
  const [uploads, setUploads] = useState<Array<{ name: string; path: string; size: number }>>([]);

  const sessionsQuery = useQuery({
    queryKey: ["chat", "sessions"],
    queryFn: () => client.sessions.list({ pageSize: 50 }).catch(() => []),
    refetchInterval: 8000,
  });

  const sessions = toArray(sessionsQuery.data, "sessions").filter(
    (row: any) => !String(row?.title || "").startsWith("__provider_test__")
  );

  useEffect(() => {
    const saved = localStorage.getItem(CHAT_SESSION_KEY) || "";
    const fallback = String(sessions[0]?.id || sessions[0]?.session_id || "");
    if (!selectedSessionId) setSelectedSessionId(saved || fallback);
  }, [selectedSessionId, sessions]);

  useEffect(() => {
    if (!selectedSessionId) return;
    localStorage.setItem(CHAT_SESSION_KEY, selectedSessionId);
  }, [selectedSessionId]);

  useEffect(() => {
    const handler = () => setSelectedSessionId("");
    window.addEventListener("tcp:new-chat", handler as EventListener);
    return () => window.removeEventListener("tcp:new-chat", handler as EventListener);
  }, []);

  const messagesQuery = useQuery({
    queryKey: ["chat", "messages", selectedSessionId],
    enabled: !!selectedSessionId,
    queryFn: () => client.sessions.messages(selectedSessionId).catch(() => ({ messages: [] })),
    refetchInterval: 2500,
  });

  const messages = toArray(messagesQuery.data, "messages");

  const createSession = useMutation({
    mutationFn: async () => {
      const id = await client.sessions.create({ title: "Control Panel Chat" });
      return String(id || "");
    },
    onSuccess: async (id) => {
      setSelectedSessionId(id);
      await queryClient.invalidateQueries({ queryKey: ["chat", "sessions"] });
    },
  });

  const sendPrompt = useMutation({
    mutationFn: async () => {
      const text = prompt.trim();
      if (!text && !uploads.length) throw new Error("Prompt is required.");
      let sessionId = selectedSessionId;
      if (!sessionId) {
        sessionId = await client.sessions.create({ title: "Control Panel Chat" });
        setSelectedSessionId(String(sessionId));
      }
      const uploadHint = uploads.length
        ? `\n\nAttached files:\n${uploads.map((f) => `- ${f.path}`).join("\n")}`
        : "";
      await client.sessions.promptAsync(
        String(sessionId),
        `${text || "Please analyze the attached files."}${uploadHint}`
      );
      return String(sessionId);
    },
    onSuccess: async (sessionId) => {
      setPrompt("");
      toast("ok", "Prompt sent.");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["chat", "sessions"] }),
        queryClient.invalidateQueries({ queryKey: ["chat", "messages", sessionId] }),
      ]);
    },
    onError: (error) => toast("err", error instanceof Error ? error.message : String(error)),
  });

  const uploadFiles = useCallback(
    async (fileList: FileList | null) => {
      const files = [...(fileList || [])];
      if (!files.length) return;
      let okCount = 0;
      for (const file of files) {
        try {
          const res = await fetch(`/api/files/upload?dir=${encodeURIComponent(CHAT_UPLOAD_DIR)}`, {
            method: "POST",
            credentials: "include",
            headers: {
              "x-file-name": encodeURIComponent(file.name),
            },
            body: file,
          });
          if (!res.ok) throw new Error(`Upload failed (${res.status})`);
          const payload = await res.json();
          setUploads((prev) => [
            {
              name: String(payload?.name || file.name),
              path: String(payload?.path || file.name),
              size: Number(payload?.size || file.size || 0),
            },
            ...prev,
          ]);
          okCount += 1;
        } catch (error) {
          toast("err", error instanceof Error ? error.message : String(error));
        }
      }
      if (okCount) toast("ok", `Uploaded ${okCount} file${okCount === 1 ? "" : "s"}.`);
    },
    [toast]
  );

  const selectedSessionTitle = useMemo(() => {
    const row = sessions.find(
      (s: any) => String(s?.id || s?.session_id || "") === selectedSessionId
    );
    return String(row?.title || "Chat");
  }, [selectedSessionId, sessions]);

  return (
    <div className="grid h-full gap-4 lg:grid-cols-[280px_1fr]">
      <PageCard title="Sessions" subtitle="Recent conversations">
        <div className="mb-2 flex gap-2">
          <button
            className="tcp-btn w-full"
            onClick={() => createSession.mutate()}
            disabled={createSession.isPending}
          >
            New Session
          </button>
          <button className="tcp-btn" onClick={() => sessionsQuery.refetch()}>
            Refresh
          </button>
        </div>
        <div className="grid max-h-[58vh] gap-2 overflow-auto pr-1">
          <AnimatePresence initial={false}>
            {sessions.map((session: any) => {
              const id = String(session?.id || session?.session_id || "");
              const active = id === selectedSessionId;
              return (
                <motion.button
                  key={id}
                  type="button"
                  onClick={() => setSelectedSessionId(id)}
                  className={`tcp-list-item text-left ${active ? "border-amber-400/60" : ""}`}
                  initial={{ opacity: 0, y: 6 }}
                  animate={{ opacity: 1, y: 0 }}
                  exit={{ opacity: 0, y: -6 }}
                >
                  <div className="truncate font-medium">
                    {String(session?.title || id || "Session")}
                  </div>
                  <div className="tcp-subtle text-xs">{id}</div>
                </motion.button>
              );
            })}
          </AnimatePresence>
          {!sessions.length ? <EmptyState text="No sessions yet." /> : null}
        </div>
      </PageCard>

      <PageCard
        title={selectedSessionTitle}
        subtitle="Send prompts, review messages, and attach files"
      >
        <div className="mb-3 grid max-h-[48vh] gap-2 overflow-auto rounded-xl border border-slate-700/60 bg-black/20 p-3">
          {messages.length ? (
            messages.map((msg: any, index: number) => {
              const role = String(msg?.role || msg?.type || "assistant");
              const content = String(msg?.content || msg?.text || msg?.message || msg?.delta || "");
              return (
                <motion.article
                  key={`${index}-${role}`}
                  className={`rounded-xl border p-3 text-sm ${role.includes("user") ? "border-sky-500/50 bg-sky-500/10" : "border-slate-700/70 bg-slate-900/50"}`}
                  initial={{ opacity: 0, y: 4 }}
                  animate={{ opacity: 1, y: 0 }}
                >
                  <div className="mb-1 text-xs uppercase tracking-wide text-slate-400">{role}</div>
                  <div className="whitespace-pre-wrap break-words text-slate-100">
                    {content || "(empty)"}
                  </div>
                </motion.article>
              );
            })
          ) : (
            <EmptyState text="No messages yet. Send a prompt to start." />
          )}
        </div>

        <div className="mb-2 flex items-center gap-2">
          <label className="tcp-btn cursor-pointer">
            Attach files
            <input
              className="hidden"
              type="file"
              multiple
              onChange={(e) => uploadFiles((e.target as HTMLInputElement).files)}
            />
          </label>
          <button
            className="tcp-btn"
            onClick={() => messagesQuery.refetch()}
            disabled={!selectedSessionId}
          >
            Refresh
          </button>
          {!!uploads.length ? (
            <span className="tcp-subtle text-xs">{uploads.length} attached</span>
          ) : null}
        </div>

        {!!uploads.length ? (
          <div className="mb-2 flex flex-wrap gap-2">
            {uploads.map((file, index) => (
              <span key={`${file.path}-${index}`} className="tcp-badge-info">
                {file.path}
                <button
                  className="ml-1 text-slate-200"
                  onClick={() => setUploads((prev) => prev.filter((_, i) => i !== index))}
                >
                  x
                </button>
              </span>
            ))}
          </div>
        ) : null}

        <div className="grid gap-2">
          <textarea
            className="tcp-input min-h-[110px]"
            placeholder="Ask anything..."
            value={prompt}
            onInput={(e) => setPrompt((e.target as HTMLTextAreaElement).value)}
          />
          <button
            className="tcp-btn-primary"
            onClick={() => sendPrompt.mutate()}
            disabled={sendPrompt.isPending}
          >
            {sendPrompt.isPending ? "Sending..." : "Send"}
          </button>
        </div>
      </PageCard>
    </div>
  );
}
