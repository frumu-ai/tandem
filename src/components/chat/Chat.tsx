import { useState, useRef, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Message, type MessageProps } from "./Message";
import { ChatInput, type FileAttachment } from "./ChatInput";
import { ActivityDrawer, type ActivityItem } from "./ActivityDrawer";
import {
  PermissionToastContainer,
  type PermissionRequest,
} from "@/components/permissions/PermissionToast";
import { FolderOpen, Sparkles, AlertCircle, Loader2 } from "lucide-react";
import {
  startSidecar,
  getSidecarStatus,
  createSession,
  sendMessageStreaming,
  cancelGeneration,
  onSidecarEvent,
  approveTool,
  denyTool,
  getSessionMessages,
  type StreamEvent,
  type SidecarState,
  type FileAttachmentInput,
} from "@/lib/tauri";

interface ChatProps {
  workspacePath: string | null;
  sessionId?: string | null;
  onSessionCreated?: (sessionId: string) => void;
  onSidecarConnected?: () => void;
}

export function Chat({
  workspacePath,
  sessionId: propSessionId,
  onSessionCreated,
  onSidecarConnected,
}: ChatProps) {
  const [messages, setMessages] = useState<MessageProps[]>([]);
  const [isGenerating, setIsGenerating] = useState(false);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(propSessionId || null);
  const [sidecarStatus, setSidecarStatus] = useState<SidecarState>("stopped");
  const [error, setError] = useState<string | null>(null);
  const [isConnecting, setIsConnecting] = useState(false);
  const [pendingPermissions, setPendingPermissions] = useState<PermissionRequest[]>([]);
  const [activities, setActivities] = useState<ActivityItem[]>([]);
  const [isLoadingHistory, setIsLoadingHistory] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const currentAssistantMessageRef = useRef<string>("");
  const generationTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastEventAtRef = useRef<number | null>(null);

  // Auto-scroll to bottom
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  // Store callback in ref to avoid dependency issues
  const onSidecarConnectedRef = useRef(onSidecarConnected);
  useEffect(() => {
    onSidecarConnectedRef.current = onSidecarConnected;
  }, [onSidecarConnected]);

  // Auto-connect sidecar on mount
  useEffect(() => {
    const autoConnect = async () => {
      try {
        const status = await getSidecarStatus();
        setSidecarStatus(status);

        // Auto-start if not already running
        if (status !== "running") {
          setIsConnecting(true);
          try {
            await startSidecar();
            setSidecarStatus("running");
            // Notify parent that sidecar is connected
            onSidecarConnectedRef.current?.();
          } catch (e) {
            console.error("Failed to auto-start sidecar:", e);
            // Don't set error - user can still manually connect
          } finally {
            setIsConnecting(false);
          }
        } else {
          // Already running, notify parent
          onSidecarConnectedRef.current?.();
        }
      } catch (e) {
        console.error("Failed to get sidecar status:", e);
      }
    };
    autoConnect();
  }, []); // Only run on mount

  // Sync internal session ID with prop
  useEffect(() => {
    setCurrentSessionId(propSessionId || null);
  }, [propSessionId]);

  // Load session history when sessionId changes from props
  useEffect(() => {
    if (propSessionId) {
      console.log("[Chat] Loading session history for:", propSessionId);
      setMessages([]);
      setActivities([]);
      currentAssistantMessageRef.current = "";
      loadSessionHistory(propSessionId);
    } else {
      // Clear messages when no session selected
      setMessages([]);
      setActivities([]);
      currentAssistantMessageRef.current = "";
    }
  }, [propSessionId]);

  const loadSessionHistory = async (sessionId: string) => {
    setIsLoadingHistory(true);
    try {
      const sessionMessages = await getSessionMessages(sessionId);

      // Convert session messages to our format
      const convertedMessages: MessageProps[] = [];

      for (const msg of sessionMessages) {
        const role = msg.info.role as "user" | "assistant" | "system";

        // Extract text content from parts
        let content = "";
        const toolCalls: MessageProps["toolCalls"] = [];

        for (const part of msg.parts) {
          const partObj = part as Record<string, unknown>;
          if (partObj.type === "text" && partObj.text) {
            content += partObj.text as string;
          } else if (partObj.type === "tool" || partObj.type === "tool-invocation") {
            const state = partObj.state as Record<string, unknown> | undefined;
            toolCalls.push({
              id: (partObj.id || partObj.callID || "") as string,
              tool: (partObj.tool || "unknown") as string,
              args: (state?.input || partObj.args || {}) as Record<string, unknown>,
              result: state?.output ? String(state.output) : undefined,
              status:
                state?.status === "completed"
                  ? "completed"
                  : state?.status === "failed"
                    ? "failed"
                    : "pending",
            });
          }
        }

        if (content || toolCalls.length > 0 || role === "user") {
          convertedMessages.push({
            id: msg.info.id,
            role,
            content,
            timestamp: new Date(msg.info.time.created),
            toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
          });
        }
      }

      setMessages(convertedMessages);
    } catch (e) {
      console.error("Failed to load session history:", e);
      setError("Failed to load chat history");
    } finally {
      setIsLoadingHistory(false);
    }
  };

  // Helper to determine activity type from tool name
  const getActivityType = (tool: string): ActivityItem["type"] => {
    const toolLower = tool.toLowerCase();
    if (toolLower.includes("read") || toolLower.includes("view")) return "file_read";
    if (toolLower.includes("write") || toolLower.includes("edit") || toolLower.includes("create"))
      return "file_write";
    if (toolLower.includes("search") || toolLower.includes("grep") || toolLower.includes("find"))
      return "search";
    if (
      toolLower.includes("bash") ||
      toolLower.includes("shell") ||
      toolLower.includes("command") ||
      toolLower.includes("exec")
    )
      return "command";
    if (toolLower.includes("browse") || toolLower.includes("web") || toolLower.includes("fetch"))
      return "browse";
    return "tool";
  };

  // Helper to get a friendly title for a tool
  const getActivityTitle = (tool: string, args: Record<string, unknown>): string => {
    const toolLower = tool.toLowerCase();

    // Try to extract a meaningful path or query
    const path = args.path || args.file || args.filename;
    const query = args.query || args.pattern || args.search;
    const command = args.command || args.cmd;

    if (path && typeof path === "string") {
      // Shorten long paths
      const shortPath = path.length > 40 ? "..." + path.slice(-37) : path;
      if (toolLower.includes("read")) return `Reading ${shortPath}`;
      if (toolLower.includes("write") || toolLower.includes("edit")) return `Editing ${shortPath}`;
      if (toolLower.includes("create")) return `Creating ${shortPath}`;
      if (toolLower.includes("delete")) return `Deleting ${shortPath}`;
      if (toolLower.includes("list")) return `Listing ${shortPath}`;
      return `${tool} → ${shortPath}`;
    }

    if (query && typeof query === "string") {
      const shortQuery = query.length > 30 ? query.slice(0, 27) + "..." : query;
      return `Searching: "${shortQuery}"`;
    }

    if (command && typeof command === "string") {
      const shortCmd = command.length > 30 ? command.slice(0, 27) + "..." : command;
      return `Running: ${shortCmd}`;
    }

    // Fallback to tool name
    return tool.replace(/_/g, " ").replace(/\b\w/g, (l) => l.toUpperCase());
  };

  const handleStreamEvent = useCallback(
    (event: StreamEvent) => {
      // Debug: Log all events
      console.log(
        "[StreamEvent]",
        event.type,
        "session:",
        (event as { session_id?: string }).session_id,
        "current:",
        currentSessionId,
        event
      );

      // Filter events for the current session
      const eventSessionId = (event as { session_id?: string }).session_id;
      if (eventSessionId && currentSessionId && eventSessionId !== currentSessionId) {
        console.log(
          "[StreamEvent] Ignoring event for different session:",
          eventSessionId,
          "!==",
          currentSessionId
        );
        return;
      }

      lastEventAtRef.current = Date.now();
      if (generationTimeoutRef.current) {
        clearTimeout(generationTimeoutRef.current);
      }
      generationTimeoutRef.current = setTimeout(() => {
        if (isGenerating) {
          setError("Response timed out. Try again or stop and restart the chat.");
          setIsGenerating(false);
          currentAssistantMessageRef.current = "";
        }
      }, 60000);

      switch (event.type) {
        case "content": {
          // Prefer full content when available to avoid duplicate appends
          const newContent = event.delta || event.content;
          console.log("[StreamEvent] Content update:", newContent?.slice(0, 100));
          if (event.delta && event.content) {
            // OpenCode often sends full content alongside delta
            // Use full content to prevent repeated text loops
            currentAssistantMessageRef.current = event.content;
          } else if (event.delta) {
            // Append delta to current message
            currentAssistantMessageRef.current += newContent;
          } else {
            // Replace with full content
            currentAssistantMessageRef.current = newContent;
          }
          setMessages((prev) => {
            const lastMessage = prev[prev.length - 1];
            if (lastMessage && lastMessage.role === "assistant") {
              return [
                ...prev.slice(0, -1),
                { ...lastMessage, content: currentAssistantMessageRef.current },
              ];
            }
            return prev;
          });
          break;
        }

        case "tool_start": {
          const args = event.args as Record<string, unknown>;

          // Add to activity panel
          const activityType = getActivityType(event.tool);
          const activityTitle = getActivityTitle(event.tool, args);

          setActivities((prev) => {
            // Check if activity already exists (update) or is new (add)
            const existingIdx = prev.findIndex((a) => a.id === event.part_id);
            if (existingIdx >= 0) {
              const updated = [...prev];
              updated[existingIdx] = {
                ...updated[existingIdx],
                status: "running",
              };
              return updated;
            }
            return [
              ...prev,
              {
                id: event.part_id,
                type: activityType,
                tool: event.tool,
                title: activityTitle,
                detail: (args.path as string) || (args.query as string) || undefined,
                status: "running",
                timestamp: new Date(),
                args,
              },
            ];
          });

          // Add tool call to the message
          setMessages((prev) => {
            const lastMessage = prev[prev.length - 1];
            if (lastMessage && lastMessage.role === "assistant") {
              const toolCalls = lastMessage.toolCalls || [];
              // Check if tool already exists (update) or is new (add)
              const existingIdx = toolCalls.findIndex((tc) => tc.id === event.part_id);
              if (existingIdx >= 0) {
                // Update existing
                const newToolCalls = [...toolCalls];
                newToolCalls[existingIdx] = {
                  ...newToolCalls[existingIdx],
                  tool: event.tool,
                  args: event.args as Record<string, unknown>,
                  status: "pending" as const,
                };
                return [...prev.slice(0, -1), { ...lastMessage, toolCalls: newToolCalls }];
              } else {
                // Add new
                return [
                  ...prev.slice(0, -1),
                  {
                    ...lastMessage,
                    toolCalls: [
                      ...toolCalls,
                      {
                        id: event.part_id,
                        tool: event.tool,
                        args: event.args as Record<string, unknown>,
                        status: "pending" as const,
                      },
                    ],
                  },
                ];
              }
            }
            return prev;
          });

          // Create permission request for destructive operations
          const needsApproval = [
            "write_file",
            "create_file",
            "delete_file",
            "run_command",
            "bash",
            "shell",
          ].includes(event.tool);
          if (needsApproval) {
            const permissionRequest: PermissionRequest = {
              id: event.part_id,
              type: event.tool as PermissionRequest["type"],
              path: args.path as string | undefined,
              command: (args.command || args.cmd) as string | undefined,
              reasoning: (args.reasoning as string) || "AI wants to perform this action",
              riskLevel:
                event.tool === "delete_file" ||
                event.tool === "run_command" ||
                event.tool === "bash"
                  ? "high"
                  : "medium",
            };
            setPendingPermissions((prev) => [...prev, permissionRequest]);
          }
          break;
        }

        case "tool_end": {
          // Update activity status
          const resultStr =
            event.error || (event.result ? JSON.stringify(event.result).slice(0, 500) : "");
          setActivities((prev) =>
            prev.map((a) =>
              a.id === event.part_id
                ? {
                    ...a,
                    status: event.error ? "failed" : "completed",
                    result: resultStr,
                  }
                : a
            )
          );

          // Update tool call with result
          setMessages((prev) => {
            const lastMessage = prev[prev.length - 1];
            if (lastMessage && lastMessage.role === "assistant" && lastMessage.toolCalls) {
              const toolCalls = lastMessage.toolCalls.map((tc) =>
                tc.id === event.part_id
                  ? {
                      ...tc,
                      result: event.error || String(event.result || ""),
                      status: (event.error ? "failed" : "completed") as "failed" | "completed",
                    }
                  : tc
              );
              return [...prev.slice(0, -1), { ...lastMessage, toolCalls }];
            }
            return prev;
          });
          break;
        }

        case "session_status":
          // Could update UI to show session status
          console.log("Session status:", event.status);
          break;

        case "session_idle":
          // Generation complete - ensure final message content is saved before clearing
          setMessages((prev) => {
            const lastMessage = prev[prev.length - 1];
            if (
              lastMessage &&
              lastMessage.role === "assistant" &&
              currentAssistantMessageRef.current
            ) {
              // Ensure the final content is applied
              return [
                ...prev.slice(0, -1),
                { ...lastMessage, content: currentAssistantMessageRef.current },
              ];
            }
            return prev;
          });

          // Mark any remaining running activities as completed
          setActivities((prev) =>
            prev.map((a) => (a.status === "running" ? { ...a, status: "completed" } : a))
          );
          setIsGenerating(false);
          currentAssistantMessageRef.current = "";
          if (generationTimeoutRef.current) {
            clearTimeout(generationTimeoutRef.current);
            generationTimeoutRef.current = null;
          }
          break;

        case "session_error":
          setError(event.error);
          setIsGenerating(false);
          currentAssistantMessageRef.current = "";
          if (generationTimeoutRef.current) {
            clearTimeout(generationTimeoutRef.current);
            generationTimeoutRef.current = null;
          }
          break;

        case "permission_asked": {
          // Handle permission requests from OpenCode
          const permissionRequest: PermissionRequest = {
            id: event.request_id,
            type: (event.tool || "unknown") as PermissionRequest["type"],
            path: event.args?.path as string | undefined,
            command: event.args?.command as string | undefined,
            reasoning: "AI requests permission to perform this action",
            riskLevel: event.tool === "delete_file" || event.tool === "bash" ? "high" : "medium",
          };
          setPendingPermissions((prev) => [...prev, permissionRequest]);
          break;
        }

        case "raw": {
          // Try to extract useful activity info from raw events
          const data = event.data as Record<string, unknown>;

          // Handle message.updated events - these often contain tool info
          if (event.event_type === "message.updated") {
            const info = data.info as Record<string, unknown> | undefined;
            if (info) {
              console.log("Message updated:", info);
            }
          }

          // Log other raw events for debugging
          console.log("Raw event:", event.event_type, data);
          break;
        }
      }
    },
    [isGenerating]
  );

  // Listen for sidecar events
  useEffect(() => {
    let unlistenFn: (() => void) | null = null;

    const setupListener = async () => {
      unlistenFn = await onSidecarEvent((event: StreamEvent) => {
        handleStreamEvent(event);
      });
    };

    setupListener();

    return () => {
      if (unlistenFn) {
        unlistenFn();
      }
    };
  }, [handleStreamEvent]);

  const connectSidecar = async () => {
    setIsConnecting(true);
    setError(null);

    try {
      await startSidecar();
      setSidecarStatus("running");
      // Don't create a session here - it will be created when user sends first message
      // Notify parent that sidecar is connected
      onSidecarConnected?.();
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e);
      setError(`Failed to start AI: ${errorMessage}`);
      setSidecarStatus("failed");
    } finally {
      setIsConnecting(false);
    }
  };

  const handleSend = async (content: string, attachments?: FileAttachment[]) => {
    setError(null);

    // If sidecar isn't running, try to start it
    let currentStatus = sidecarStatus;
    if (currentStatus !== "running") {
      try {
        await connectSidecar();
        currentStatus = await getSidecarStatus();
      } catch (e) {
        console.error("Failed to connect:", e);
        return;
      }
      if (currentStatus !== "running") {
        return;
      }
    }

    // Create session if needed
    let sessionId = currentSessionId;
    if (!sessionId) {
      try {
        const session = await createSession();
        sessionId = session.id;
        setCurrentSessionId(session.id);
        onSessionCreated?.(session.id);
      } catch (e) {
        setError(`Failed to create session: ${e}`);
        return;
      }
    }

    // Add user message
    const userMessage: MessageProps = {
      id: crypto.randomUUID(),
      role: "user",
      content,
      timestamp: new Date(),
      // Show attachments in message if any
      attachments: attachments?.map((a) => ({
        name: a.name,
        type: a.type,
        preview: a.preview,
      })),
    };
    setMessages((prev) => [...prev, userMessage]);

    // Add placeholder assistant message
    const assistantMessage: MessageProps = {
      id: crypto.randomUUID(),
      role: "assistant",
      content: "",
      timestamp: new Date(),
    };
    setMessages((prev) => [...prev, assistantMessage]);
    setIsGenerating(true);
    currentAssistantMessageRef.current = "";
    lastEventAtRef.current = Date.now();
    if (generationTimeoutRef.current) {
      clearTimeout(generationTimeoutRef.current);
    }
    generationTimeoutRef.current = setTimeout(() => {
      if (lastEventAtRef.current && Date.now() - lastEventAtRef.current >= 60000) {
        setError("Response timed out. Try again or stop and restart the chat.");
        setIsGenerating(false);
        currentAssistantMessageRef.current = "";
      }
    }, 60000);

    try {
      // Convert attachments to API format
      const attachmentInputs: FileAttachmentInput[] | undefined = attachments?.map((a) => ({
        mime: a.mime,
        filename: a.name,
        url: a.url,
      }));

      // Send message and stream response
      await sendMessageStreaming(sessionId, content, attachmentInputs);
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e);
      setError(`Failed to send message: ${errorMessage}`);
      setIsGenerating(false);

      // Update the assistant message with error
      setMessages((prev) => {
        const lastMessage = prev[prev.length - 1];
        if (lastMessage && lastMessage.role === "assistant" && !lastMessage.content) {
          return [
            ...prev.slice(0, -1),
            {
              ...lastMessage,
              content: `Error: ${errorMessage}`,
            },
          ];
        }
        return prev;
      });
    }
  };

  const handleStop = async () => {
    if (currentSessionId) {
      try {
        await cancelGeneration(currentSessionId);
      } catch (e) {
        console.error("Failed to cancel generation:", e);
      }
    }
    setIsGenerating(false);
    if (generationTimeoutRef.current) {
      clearTimeout(generationTimeoutRef.current);
      generationTimeoutRef.current = null;
    }
  };

  const handleApprovePermission = async (id: string, _remember?: "once" | "session" | "always") => {
    if (!currentSessionId) return;

    try {
      await approveTool(currentSessionId, id);

      // Update tool call status
      setMessages((prev) => {
        return prev.map((msg) => {
          if (msg.role === "assistant" && msg.toolCalls) {
            return {
              ...msg,
              toolCalls: msg.toolCalls.map((tc) =>
                tc.id === id ? { ...tc, status: "running" as const } : tc
              ),
            };
          }
          return msg;
        });
      });

      // Remove from pending
      setPendingPermissions((prev) => prev.filter((p) => p.id !== id));
    } catch (e) {
      console.error("Failed to approve tool:", e);
      setError(`Failed to approve action: ${e}`);
    }
  };

  const handleDenyPermission = async (id: string, _remember?: boolean) => {
    if (!currentSessionId) return;

    try {
      await denyTool(currentSessionId, id);

      // Update tool call status
      setMessages((prev) => {
        return prev.map((msg) => {
          if (msg.role === "assistant" && msg.toolCalls) {
            return {
              ...msg,
              toolCalls: msg.toolCalls.map((tc) =>
                tc.id === id ? { ...tc, status: "failed" as const, result: "Denied by user" } : tc
              ),
            };
          }
          return msg;
        });
      });

      // Remove from pending
      setPendingPermissions((prev) => prev.filter((p) => p.id !== id));
    } catch (e) {
      console.error("Failed to deny tool:", e);
      setError(`Failed to deny action: ${e}`);
    }
  };

  const needsConnection = sidecarStatus !== "running" && !isConnecting;

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <header className="flex items-center justify-between border-b border-border px-6 py-4">
        <div className="flex items-center gap-3">
          <div className="h-10 w-10 overflow-hidden rounded-xl ring-1 ring-white/10">
            <img src="/tandem-logo.png" alt="Tandem logo" className="h-full w-full object-cover" />
          </div>
          <div>
            <h1 className="font-semibold text-text">Tandem</h1>
            {workspacePath && (
              <p className="flex items-center gap-1 text-sm text-text-muted">
                <FolderOpen className="h-3 w-3" />
                {workspacePath}
              </p>
            )}
          </div>
        </div>

        {/* Connection status */}
        <div className="flex items-center gap-2">
          <div
            className={`h-2 w-2 rounded-full ${
              sidecarStatus === "running"
                ? "bg-success"
                : sidecarStatus === "starting"
                  ? "bg-warning animate-pulse"
                  : "bg-text-subtle"
            }`}
          />
          <span className="text-xs text-text-muted">
            {sidecarStatus === "running"
              ? "Connected"
              : sidecarStatus === "starting"
                ? "Connecting..."
                : "Disconnected"}
          </span>
        </div>
      </header>

      {/* Error banner */}
      {error && (
        <div className="flex items-center gap-2 bg-error/10 px-4 py-2 text-sm text-error">
          <AlertCircle className="h-4 w-4" />
          {error}
          <button onClick={() => setError(null)} className="ml-auto text-error/70 hover:text-error">
            ×
          </button>
        </div>
      )}

      {/* Messages */}
      <div className="flex-1 overflow-y-auto pb-48">
        <AnimatePresence>
          {isLoadingHistory ? (
            <motion.div
              className="flex h-full items-center justify-center"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
            >
              <div className="flex flex-col items-center gap-3">
                <Loader2 className="h-8 w-8 animate-spin text-primary" />
                <p className="text-sm text-text-muted">Loading chat history...</p>
              </div>
            </motion.div>
          ) : messages.length === 0 ? (
            <EmptyState
              needsConnection={needsConnection}
              isConnecting={isConnecting}
              onConnect={connectSidecar}
              workspacePath={workspacePath}
            />
          ) : (
            messages.map((message) => <Message key={message.id} {...message} />)
          )}
        </AnimatePresence>

        {/* Streaming indicator */}
        {isGenerating && (
          <motion.div
            className="glass border-glass rounded-2xl shadow-lg shadow-black/20 ring-1 ring-white/5 px-4 py-6 flex gap-4"
            initial={{ opacity: 0, filter: "blur(6px)" }}
            animate={{ opacity: 1, filter: "blur(0px)" }}
            transition={{ duration: 0.2 }}
          >
            <div className="relative flex h-8 w-8 items-center justify-center rounded-lg bg-gradient-to-br from-secondary to-primary text-white shadow-[0_0_12px_rgba(59,130,246,0.45)]">
              <Sparkles className="h-4 w-4 animate-pulse" />
            </div>
            <div className="flex items-center gap-3">
              <span className="inline-block h-3 w-1.5 bg-primary animate-pulse" />
              <span className="terminal-text text-text-muted">Processing</span>
              <div className="flex gap-1">
                <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-text-subtle [animation-delay:-0.3s]" />
                <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-text-subtle [animation-delay:-0.15s]" />
                <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-text-subtle" />
              </div>
            </div>
          </motion.div>
        )}

        <div ref={messagesEndRef} />
      </div>

      {/* Input */}
      <ChatInput
        onSend={handleSend}
        onStop={handleStop}
        isGenerating={isGenerating}
        disabled={!workspacePath || isGenerating}
        placeholder={
          workspacePath
            ? needsConnection
              ? "Type to connect and start chatting..."
              : "Ask Tandem anything..."
            : "Select a workspace to start chatting"
        }
      />

      {/* Permission requests */}
      <PermissionToastContainer
        requests={pendingPermissions}
        onApprove={handleApprovePermission}
        onDeny={handleDenyPermission}
      />

      {/* Activity drawer */}
      <ActivityDrawer activities={activities} isGenerating={isGenerating} />
    </div>
  );
}

interface EmptyStateProps {
  needsConnection: boolean;
  isConnecting: boolean;
  onConnect: () => void;
  workspacePath: string | null;
}

function EmptyState({ needsConnection, isConnecting, onConnect, workspacePath }: EmptyStateProps) {
  return (
    <motion.div
      className="flex h-full flex-col items-center justify-center p-8"
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
    >
      <div className="max-w-md text-center">
        <div className="mx-auto mb-6 flex h-20 w-20 items-center justify-center rounded-2xl bg-gradient-to-br from-primary/20 to-secondary/20">
          <Sparkles className="h-10 w-10 text-primary" />
        </div>

        <h2 className="mb-3 text-2xl font-bold text-text">What can I help you with?</h2>

        <p className="mb-8 text-text-muted">
          I can read and write files, search your codebase, run commands, and help you accomplish
          tasks in your workspace.
        </p>

        {needsConnection && workspacePath && (
          <button
            onClick={onConnect}
            disabled={isConnecting}
            className="mb-8 inline-flex items-center gap-2 rounded-lg bg-primary px-6 py-3 font-medium text-white transition-colors hover:bg-primary/90 disabled:opacity-50"
          >
            {isConnecting ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                Connecting...
              </>
            ) : (
              <>
                <Sparkles className="h-4 w-4" />
                Connect AI
              </>
            )}
          </button>
        )}

        <div className="grid gap-3 text-left">
          <SuggestionCard
            title="Explore your codebase"
            description="Help me understand the structure of this project"
          />
          <SuggestionCard
            title="Refactor code"
            description="Improve the error handling in src/utils.ts"
          />
          <SuggestionCard
            title="Write documentation"
            description="Create a README for this project"
          />
        </div>
      </div>
    </motion.div>
  );
}

interface SuggestionCardProps {
  title: string;
  description: string;
}

function SuggestionCard({ title, description }: SuggestionCardProps) {
  return (
    <button className="rounded-lg border border-border bg-surface p-4 text-left transition-colors hover:border-primary/50 hover:bg-surface-elevated">
      <p className="font-medium text-text">{title}</p>
      <p className="text-sm text-text-muted">{description}</p>
    </button>
  );
}
