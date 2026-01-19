import { useState, useRef, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Message, type MessageProps } from "./Message";
import { ChatInput, type FileAttachment } from "./ChatInput";
import {
  PermissionToastContainer,
  type PermissionRequest,
} from "@/components/permissions/PermissionToast";
import { ExecutionPlanPanel } from "@/components/plan/ExecutionPlanPanel";
import { PlanActionButtons } from "./PlanActionButtons";
import { QuestionDialog } from "./QuestionDialog";
import { useStagingArea } from "@/hooks/useStagingArea";
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
  answerQuestion,
  getSessionMessages,
  undoViaCommand,
  isGitRepo,
  getToolGuidance,
  type StreamEvent,
  type SidecarState,
  type TodoItem,
  type QuestionEvent,
} from "@/lib/tauri";

interface ChatProps {
  workspacePath: string | null;
  sessionId?: string | null;
  onSessionCreated?: (sessionId: string) => void;
  onSidecarConnected?: () => void;
  usePlanMode?: boolean;
  onPlanModeChange?: (enabled: boolean) => void;
  onToggleTaskSidebar?: () => void;
  executePendingTasksTrigger?: number;
  onGeneratingChange?: (isGenerating: boolean) => void;
  pendingTasks?: TodoItem[];
  fileToAttach?: FileAttachment | null;
  onFileAttached?: () => void;
  selectedAgent?: string;
  onAgentChange?: (agent: string | undefined) => void;
  onFileOpen?: (filePath: string) => void;
}

export function Chat({
  workspacePath,
  sessionId: propSessionId,
  onSessionCreated,
  onSidecarConnected,
  usePlanMode: propUsePlanMode = false,
  onPlanModeChange,
  onToggleTaskSidebar,
  executePendingTasksTrigger,
  onGeneratingChange,
  pendingTasks,
  fileToAttach,
  onFileAttached,
  selectedAgent: propSelectedAgent,
  onAgentChange: propOnAgentChange,
  onFileOpen,
}: ChatProps) {
  const [messages, setMessages] = useState<MessageProps[]>([]);
  const [isGenerating, setIsGenerating] = useState(false);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(propSessionId || null);
  const [enabledToolCategories, setEnabledToolCategories] = useState<Set<string>>(new Set());
  const [, forceUpdate] = useState({}); // Keep for critical display updates

  // Notify parent when generating state changes
  useEffect(() => {
    onGeneratingChange?.(isGenerating);
  }, [isGenerating, onGeneratingChange]);
  const [sidecarStatus, setSidecarStatus] = useState<SidecarState>("stopped");
  const [error, setError] = useState<string | null>(null);
  const [isConnecting, setIsConnecting] = useState(false);
  const [pendingPermissions, setPendingPermissions] = useState<PermissionRequest[]>([]);
  const [pendingQuestion, setPendingQuestion] = useState<QuestionEvent | null>(null);
  // const [activities, setActivities] = useState<ActivityItem[]>([]);
  const [isLoadingHistory, setIsLoadingHistory] = useState(false);
  const [isGitRepository, setIsGitRepository] = useState(false);

  // Support both new agent prop and legacy usePlanMode
  const selectedAgent =
    propSelectedAgent !== undefined ? propSelectedAgent : propUsePlanMode ? "plan" : undefined;
  const onAgentChange =
    propOnAgentChange ||
    ((agent: string | undefined) => {
      onPlanModeChange?.(agent === "plan");
    });
  const usePlanMode = selectedAgent === "plan";
  const setUsePlanMode = (enabled: boolean) => {
    onAgentChange(enabled ? "plan" : undefined);
  };
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const currentAssistantMessageRef = useRef<string>("");
  const currentAssistantMessageIdRef = useRef<string | null>(null);
  const generationTimeoutRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastEventAtRef = useRef<number | null>(null);
  const prevPropSessionIdRef = useRef<string | null>(null);

  // Staging area hook
  const {
    stagedOperations,
    isExecuting: isExecutingPlan,
    stageOperation,
    removeOperation,
    executePlan,
    clearStaging,
  } = useStagingArea();

  // Handle execute pending tasks from sidebar
  useEffect(() => {
    if (
      executePendingTasksTrigger &&
      executePendingTasksTrigger > 0 &&
      currentSessionId &&
      !isGenerating &&
      pendingTasks &&
      pendingTasks.length > 0
    ) {
      // Build a message with actual task content - use action-oriented prompts
      const taskList = pendingTasks.map((t, i) => `${i + 1}. ${t.content}`).join("\n");

      const message = `Please implement the following tasks from our plan:

${taskList}

Start with task #1 and continue through each one. Let me know when each task is complete.`;

      // Small delay to ensure state is ready
      setTimeout(() => {
        handleSend(message);
      }, 100);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [executePendingTasksTrigger]);

  // Auto-scroll to bottom smoothly (only when new messages arrive or generation ends)
  const previousMessageCountRef = useRef(0);
  const messagesContainerRef = useRef<HTMLDivElement>(null);

  // Helper function to scroll to bottom
  const scrollToBottom = useCallback((smooth = false) => {
    window.requestAnimationFrame(() => {
      messagesEndRef.current?.scrollIntoView({
        behavior: smooth ? "smooth" : "auto",
        block: "end",
      });
    });
  }, []);

  // Scroll when message count increases (new message added)
  useEffect(() => {
    const currentCount = messages.length;

    if (currentCount > previousMessageCountRef.current) {
      scrollToBottom(false); // Instant scroll for new messages
    }

    previousMessageCountRef.current = currentCount;
  }, [messages.length, scrollToBottom]);

  // Also scroll when generation stops (final message complete)
  useEffect(() => {
    if (!isGenerating && messages.length > 0) {
      // Small delay to ensure content is rendered, then scroll
      setTimeout(() => scrollToBottom(false), 100);
    }
  }, [isGenerating, scrollToBottom]);

  // Scroll during active generation (streaming content)
  useEffect(() => {
    if (isGenerating && messages.length > 0) {
      scrollToBottom(false); // Keep scrolling as content streams
    }
  }, [messages, isGenerating, scrollToBottom]);

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

  // Sync internal session ID with prop (only when prop truly changes)
  useEffect(() => {
    const nextSessionId = propSessionId || null;
    const prevSessionId = currentSessionIdRef.current;

    // No change → do nothing (prevents churn from unrelated state updates)
    if (nextSessionId === prevSessionId) return;

    console.log("[Chat] Session ID changed from", prevSessionId, "to", nextSessionId);

    setCurrentSessionId(nextSessionId);
    currentSessionIdRef.current = nextSessionId; // Update ref synchronously

    // Reset streaming state for the new session
    setIsGenerating(false);
    currentAssistantMessageRef.current = "";
    currentAssistantMessageIdRef.current = null;

    if (generationTimeoutRef.current) {
      clearTimeout(generationTimeoutRef.current);
      generationTimeoutRef.current = null;
    }
  }, [propSessionId]);

  // Load session history when prop session changes (avoid transient mismatches)
  useEffect(() => {
    const nextSessionId = propSessionId || null;
    const prevPropSessionId = prevPropSessionIdRef.current;

    if (nextSessionId === prevPropSessionId) return;
    prevPropSessionIdRef.current = nextSessionId;

    if (nextSessionId) {
      console.log("[Chat] Loading session history for:", nextSessionId);
      setMessages([]);
      currentAssistantMessageRef.current = "";
      loadSessionHistory(nextSessionId);
    } else {
      // Switching to new chat
      console.log("[Chat] Clearing messages for new chat");
      setMessages([]);
      currentAssistantMessageRef.current = "";
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [propSessionId]);

  // Check if workspace is a Git repository
  useEffect(() => {
    const checkGitRepo = async () => {
      if (workspacePath) {
        try {
          const isGit = await isGitRepo(workspacePath);
          setIsGitRepository(isGit);
          console.log(
            "[Chat] Workspace is Git repo:",
            isGit,
            "- Undo button will be",
            isGit ? "visible" : "hidden"
          );
        } catch (e) {
          console.error("[Chat] Failed to check Git status:", e);
          setIsGitRepository(false);
        }
      } else {
        setIsGitRepository(false);
      }
    };
    checkGitRepo();
  }, [workspacePath]);

  const loadSessionHistory = useCallback(
    async (sessionId: string) => {
      setIsLoadingHistory(true);
      try {
        // Check if sidecar is running before attempting to load
        const status = await getSidecarStatus();
        if (status !== "running") {
          console.log("[LoadHistory] Sidecar not running, skipping history load");
          setIsLoadingHistory(false);
          return;
        }

        const sessionMessages = await getSessionMessages(sessionId);

        console.log("[LoadHistory] Received", sessionMessages.length, "messages from OpenCode");

        // Log FULL message structure to see all available fields
        if (sessionMessages.length > 0) {
          console.log(
            "[LoadHistory] FULL message structure sample:",
            JSON.stringify(sessionMessages[0], null, 2)
          );
        }

        console.log(
          "[LoadHistory] Messages with flags:",
          sessionMessages.map((m) => ({
            id: m.info.id,
            role: m.info.role,
            reverted: m.info.reverted,
            deleted: m.info.deleted,
            hasRevertedFlag: "reverted" in m.info,
            hasDeletedFlag: "deleted" in m.info,
          }))
        );

        // Convert session messages to our format
        const convertedMessages: MessageProps[] = [];

        for (const msg of sessionMessages) {
          // Skip reverted or deleted messages
          if (msg.info.reverted || msg.info.deleted) {
            console.log(`[LoadHistory] Skipping reverted/deleted message: ${msg.info.id}`);
            continue;
          }

          const role = msg.info.role as "user" | "assistant" | "system";

          // Extract text content from parts
          let content = "";
          const toolCalls: MessageProps["toolCalls"] = [];
          const attachments: { name: string; type: string }[] = [];

          for (const part of msg.parts) {
            const partObj = part as Record<string, unknown>;
            if (partObj.type === "text" && partObj.text) {
              content += partObj.text as string;
            } else if (partObj.type === "file") {
              // Handle file attachments - add a placeholder in content
              const filename = (partObj.filename as string) || "file";
              const mime = (partObj.mime as string) || "application/octet-stream";
              attachments.push({ name: filename, type: mime });
              content += `\n[📎 Attached file: ${filename}]\n`;
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

          // Only add messages that have actual text content or are user messages
          // Skip assistant messages that only have tool calls (internal OpenCode operations)
          if (content || role === "user") {
            convertedMessages.push({
              id: msg.info.id,
              role,
              content,
              timestamp: new Date(msg.info.time.created),
              toolCalls: toolCalls.length > 0 ? toolCalls : undefined,
            });
          }
        }

        console.log("[LoadHistory] Converted to", convertedMessages.length, "UI messages");
        setMessages(convertedMessages);
        // Force re-render to ensure messages display correctly
        setTimeout(() => forceUpdate({}), 50);
      } catch (e) {
        console.error("Failed to load session history:", e);
        setError("Failed to load chat history");
      } finally {
        setIsLoadingHistory(false);
      }
    },
    [getSidecarStatus, getSessionMessages, setError, setIsLoadingHistory, setMessages]
  );

  // Helper to determine activity type from tool name - DISABLED
  // const getActivityType = (tool: string): ActivityItem["type"] => {
  //   const toolLower = tool.toLowerCase();
  //   if (toolLower.includes("read") || toolLower.includes("view")) return "file_read";
  //   if (toolLower.includes("write") || toolLower.includes("edit") || toolLower.includes("create"))
  //     return "file_write";
  //   if (toolLower.includes("search") || toolLower.includes("grep") || toolLower.includes("find"))
  //     return "search";
  //   if (
  //     toolLower.includes("bash") ||
  //     toolLower.includes("shell") ||
  //     toolLower.includes("command") ||
  //     toolLower.includes("exec")
  //   )
  //     return "command";
  //   if (toolLower.includes("browse") || toolLower.includes("web") || toolLower.includes("fetch"))
  //     return "browse";
  //   return "tool";
  // };

  // Helper to get a friendly title for a tool - DISABLED
  // const getActivityTitle = (tool: string, args: Record<string, unknown>): string => {
  //   const toolLower = tool.toLowerCase();

  //   // Try to extract a meaningful path or query
  //   const path = args.path || args.file || args.filename;
  //   const query = args.query || args.pattern || args.search;
  //   const command = args.command || args.cmd;

  //   if (path && typeof path === "string") {
  //     // Shorten long paths
  //     const shortPath = path.length > 40 ? "..." + path.slice(-37) : path;
  //     if (toolLower.includes("read")) return `Reading ${shortPath}`;
  //     if (toolLower.includes("write") || toolLower.includes("edit")) return `Editing ${shortPath}`;
  //     if (toolLower.includes("create")) return `Creating ${shortPath}`;
  //     if (toolLower.includes("delete")) return `Deleting ${shortPath}`;
  //     if (toolLower.includes("list")) return `Listing ${shortPath}`;
  //     return `${tool} → ${shortPath}`;
  //   }

  //   if (query && typeof query === "string") {
  //     const shortQuery = query.length > 30 ? query.slice(0, 27) + "..." : query;
  //     return `Searching: "${shortQuery}"`;
  //   }

  //   if (command && typeof command === "string") {
  //     const shortCmd = command.length > 30 ? command.slice(0, 27) + "..." : command;
  //     return `Running: ${shortCmd}`;
  //   }

  //   // Fallback to tool name
  //   return tool.replace(/_/g, " ").replace(/\b\w/g, (l) => l.toUpperCase());
  // };

  // Use a ref to track the current session without causing re-renders
  const currentSessionIdRef = useRef<string | null>(null);

  // Update ref when session changes (but this doesn't cause handleStreamEvent to recreate)
  useEffect(() => {
    currentSessionIdRef.current = currentSessionId;
  }, [currentSessionId]);

  const handleStreamEvent = useCallback(
    (event: StreamEvent) => {
      // Debug: Log all events
      console.log(
        "[StreamEvent]",
        event.type,
        "session:",
        (event as { session_id?: string }).session_id,
        "current:",
        currentSessionIdRef.current,
        event
      );

      // Filter events for the current session
      // IMPORTANT: Use ref value to avoid recreating this callback
      const eventSessionId = (event as { session_id?: string }).session_id;
      if (
        eventSessionId &&
        currentSessionIdRef.current &&
        eventSessionId !== currentSessionIdRef.current
      ) {
        console.log(
          "[StreamEvent] Ignoring event for different session:",
          eventSessionId,
          "!==",
          currentSessionIdRef.current
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

          // Update the message ID ref if we have one from OpenCode
          if (event.message_id && !currentAssistantMessageIdRef.current) {
            currentAssistantMessageIdRef.current = event.message_id;
            console.log("[StreamEvent] Set assistant message ID:", event.message_id);
          }

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
            const targetId = currentAssistantMessageIdRef.current;
            const contentToApply = currentAssistantMessageRef.current;

            // 1) If we have a stable OpenCode message id, prefer updating that message
            if (targetId) {
              const idx = prev.findIndex((m) => m.id === targetId);
              if (idx >= 0) {
                const updated = [...prev];
                updated[idx] = { ...updated[idx], content: contentToApply };
                return updated;
              }
            }

            // 2) Otherwise, update the last assistant message if present
            const lastMessage = prev[prev.length - 1];
            if (lastMessage && lastMessage.role === "assistant") {
              return [
                ...prev.slice(0, -1),
                {
                  ...lastMessage,
                  id: targetId || lastMessage.id,
                  content: contentToApply,
                },
              ];
            }

            // 3) If placeholder assistant message is missing (e.g., cleared by a session effect),
            // append a new assistant message so content isn't lost.
            return [
              ...prev,
              {
                id: targetId || crypto.randomUUID(),
                role: "assistant",
                content: contentToApply,
                timestamp: new Date(),
              },
            ];
          });
          break;
        }

        case "tool_start": {
          const args = event.args as Record<string, unknown>;

          // Add to activity panel - DISABLED
          // const activityType = getActivityType(event.tool);
          // const activityTitle = getActivityTitle(event.tool, args);

          // setActivities((prev) => {
          //   // Check if activity already exists (update) or is new (add)
          //   const existingIdx = prev.findIndex((a) => a.id === event.part_id);
          //   if (existingIdx >= 0) {
          //     const updated = [...prev];
          //     updated[existingIdx] = {
          //       ...updated[existingIdx],
          //       status: "running",
          //     };
          //     return updated;
          //   }
          //   return [
          //     ...prev,
          //     {
          //       id: event.part_id,
          //       type: activityType,
          //       tool: event.tool,
          //       title: activityTitle,
          //       detail: (args.path as string) || (args.query as string) || undefined,
          //       status: "running",
          //       timestamp: new Date(),
          //       args,
          //     },
          //   ];
          // });

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
              tool: event.tool,
              args,
              messageId: event.message_id,
            };
            setPendingPermissions((prev) => [...prev, permissionRequest]);
          }
          break;
        }

        case "tool_end": {
          // Update activity status - DISABLED
          // const resultStr =
          //   event.error || (event.result ? JSON.stringify(event.result).slice(0, 500) : "");
          // setActivities((prev) =>
          //   prev.map((a) =>
          //     a.id === event.part_id
          //       ? {
          //           ...a,
          //           status: event.error ? "failed" : "completed",
          //           result: resultStr,
          //         }
          //       : a
          //   )
          // );

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
              // Ensure the final content is applied with the correct ID
              return [
                ...prev.slice(0, -1),
                {
                  ...lastMessage,
                  id: currentAssistantMessageIdRef.current || lastMessage.id,
                  content: currentAssistantMessageRef.current,
                },
              ];
            }
            return prev;
          });

          // Mark any remaining running activities as completed - DISABLED
          // setActivities((prev) =>
          //   prev.map((a) => (a.status === "running" ? { ...a, status: "completed" } : a))
          // );
          setIsGenerating(false);
          currentAssistantMessageRef.current = "";
          currentAssistantMessageIdRef.current = null; // Reset the ID ref
          if (generationTimeoutRef.current) {
            clearTimeout(generationTimeoutRef.current);
            generationTimeoutRef.current = null;
          }
          // Force re-render to ensure final content displays
          setTimeout(() => forceUpdate({}), 50);
          break;

        case "session_error":
          console.error("[StreamEvent] Session error:", event.error);

          // Display the error to the user
          setError(`Session error: ${event.error}`);

          // Stop generation and clean up
          setIsGenerating(false);
          currentAssistantMessageRef.current = "";

          // Clear generation timeout
          if (generationTimeoutRef.current) {
            clearTimeout(generationTimeoutRef.current);
            generationTimeoutRef.current = null;
          }

          // Update the last assistant message with the error if it exists
          setMessages((prev) => {
            const lastMessage = prev[prev.length - 1];
            if (lastMessage && lastMessage.role === "assistant" && !lastMessage.content) {
              const updated = [...prev];
              updated[updated.length - 1] = {
                ...lastMessage,
                content: `Error: ${event.error}`,
              };
              return updated;
            }
            return prev;
          });
          break;

        case "permission_asked": {
          // Handle permission requests from OpenCode
          // Use the current assistant message ID so we can associate file snapshots with this message
          const currentMsgId = currentAssistantMessageIdRef.current;
          console.log(
            "[Permission] Asked for tool:",
            event.tool,
            "messageId:",
            currentMsgId,
            "args:",
            event.args
          );

          // Check if this is a destructive operation
          const isDestructive = [
            "write",
            "write_file",
            "create_file",
            "delete",
            "delete_file",
            "bash",
            "shell",
            "run_command",
          ].includes(event.tool || "");

          // Route to staging if plan mode is enabled and operation is destructive
          if (usePlanMode && isDestructive && currentSessionId) {
            console.log("[Permission] Routing to staging area");
            stageOperation(
              event.request_id,
              currentSessionId,
              event.tool || "unknown",
              (event.args as Record<string, unknown>) || {},
              currentMsgId || undefined
            ).catch((err) => {
              console.error("[Permission] Failed to stage operation:", err);
              setError(`Failed to stage operation: ${err}`);
            });
          } else {
            // Immediate mode: show permission toast as before
            const permissionRequest: PermissionRequest = {
              id: event.request_id,
              type: (event.tool || "unknown") as PermissionRequest["type"],
              path: event.args?.path as string | undefined,
              command: event.args?.command as string | undefined,
              reasoning: "AI requests permission to perform this action",
              riskLevel: event.tool === "delete_file" || event.tool === "bash" ? "high" : "medium",
              tool: event.tool || undefined,
              args: (event.args as Record<string, unknown>) || undefined,
              messageId: currentMsgId || undefined, // Associate with current message for undo
            };
            setPendingPermissions((prev) => [...prev, permissionRequest]);
          }
          break;
        }

        case "question_asked": {
          const questionEvent: QuestionEvent = {
            session_id: event.session_id,
            question_id: event.question_id,
            header: event.header,
            question: event.question,
            options: event.options,
          };
          setPendingQuestion(questionEvent);
          break;
        }

        case "raw": {
          // Try to extract useful activity info from raw events
          const data = event.data as Record<string, unknown>;

          // NOTE: We intentionally DON'T handle message.removed events here
          // because it causes issues with OpenCode auto-generating responses
          // when the conversation ends on a user message. We handle message
          // removal optimistically in handleUndo instead.

          // Handle message.updated events - these often contain tool info
          if (event.event_type === "message.updated") {
            const info = data.info as Record<string, unknown> | undefined;
            if (info) {
              console.log("Message updated:", info);
            }
          }

          // Log todo events for debugging
          if (event.event_type === "todo.updated") {
            console.log("[Todo] Received todo.updated event:", data);
          }

          // Log other raw events for debugging
          console.log("Raw event:", event.event_type, data);
          break;
        }
      }
    },
    [isGenerating] // Using ref for currentSessionId, so no need to include it
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

  const handleSend = useCallback(
    async (content: string, attachments?: FileAttachment[], forceMode?: "plan" | "immediate") => {
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
          currentSessionIdRef.current = session.id; // Update ref synchronously before events arrive
          onSessionCreated?.(session.id);
        } catch (e) {
          setError(`Failed to create session: ${e}`);
          return;
        }
      }

      // Select agent
      // Override agent if forceMode is specified (ensures OpenCode uses correct mode)
      const agentToUse =
        forceMode === "immediate" ? undefined : forceMode === "plan" ? "plan" : selectedAgent;

      // Inject tool guidance if tool categories are enabled
      let finalContent = content;
      if (enabledToolCategories.size > 0) {
        try {
          const guidance = await getToolGuidance(Array.from(enabledToolCategories));

          if (guidance.length > 0) {
            // Determine effective mode once for all guidance
            const effectiveMode =
              forceMode === "immediate" ? false : forceMode === "plan" ? true : usePlanMode;

            const guidanceText = guidance
              .map((g) => {
                let instructions = g.instructions;

                if (!effectiveMode) {
                  // In Immediate Mode: Skip the planning phase, just create directly
                  instructions = instructions.replace(
                    /PHASE 1: PLANNING.*?PHASE 2: EXECUTION \(After User Approval\)/s,
                    "When user requests a presentation, create the .tandem.ppt.json file directly using the write tool."
                  );
                }

                return `
=== ${g.category.toUpperCase()} CAPABILITY ENABLED ===

${instructions}

JSON Schema:
${JSON.stringify(g.json_schema, null, 2)}

Example:
${g.example}
`;
              })
              .join("\n\n");

            finalContent = `${guidanceText}\n\n===== USER REQUEST =====\n${content}`;
            console.log(
              `[ToolGuidance] Injected ${effectiveMode ? "Plan Mode" : "Immediate Mode"} guidance for: ${Array.from(enabledToolCategories).join(", ")}`
            );
          }
        } catch (e) {
          console.error("Failed to get tool guidance:", e);
          // Continue without guidance
        }
      }

      // In Plan Mode, guide the AI to use todowrite for task tracking
      // Use effective mode (respect forceMode override)
      const effectivePlanMode =
        forceMode === "immediate" ? false : forceMode === "plan" ? true : usePlanMode;

      if (effectivePlanMode) {
        finalContent = `${finalContent}

(Please use the todowrite tool to create a structured task list for tracking this work, then explain your plan.)`;
        console.log("[PlanMode] Using OpenCode's Plan agent with todowrite guidance");
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
      currentAssistantMessageIdRef.current = null; // Reset the message ID ref
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
        // For data URLs, embed content directly in the message instead of using file attachments
        // to avoid OpenCode's buggy file download behavior
        let messageContent = finalContent;
        if (attachments && attachments.length > 0) {
          for (const attachment of attachments) {
            if (attachment.url.startsWith("data:")) {
              // Decode the data URL and embed it in the message
              const base64Data = attachment.url.split(",")[1];
              const decodedContent = decodeURIComponent(
                // eslint-disable-next-line no-undef
                escape(atob(base64Data))
              );

              // Use a generic format that won't trigger OpenCode to look for files
              messageContent += `\n\nHere is the attached content:\n\`\`\`\n${decodedContent}\n\`\`\`\n`;
            }
          }
        }

        // Don't send file attachments for data URLs - content is now embedded
        // Send message and stream response, with selected agent
        await sendMessageStreaming(sessionId, messageContent, undefined, agentToUse);
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
    },
    [
      sidecarStatus,
      currentSessionId,
      onSessionCreated,
      connectSidecar,
      getSidecarStatus,
      createSession,
      sendMessageStreaming,
      setError,
      setIsGenerating,
      setMessages,
      usePlanMode,
      selectedAgent,
      enabledToolCategories,
      stagedOperations.length,
    ]
  );

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

  const handleUndo = useCallback(
    async (_messageId: string) => {
      if (!currentSessionId) return;

      try {
        console.log("[Undo] Executing /undo command for session:", currentSessionId);

        // Execute the /undo command which triggers Git-based file restoration
        // Note: OpenCode's /undo operates on the entire session, not individual messages
        await undoViaCommand(currentSessionId);

        console.log("[Undo] Command executed successfully, reloading session history");

        // Reload the session history to reflect the reverted state
        await loadSessionHistory(currentSessionId);

        console.log("[Undo] Session reloaded successfully");
      } catch (e) {
        const errorMessage = e instanceof Error ? e.message : String(e);
        setError(`Failed to undo: ${errorMessage}`);
        console.error("[Undo] Error:", e);
      }
    },
    [currentSessionId]
  );

  const handleEdit = useCallback(
    async (messageId: string, newContent: string) => {
      // Find this user message
      const msgIndex = messages.findIndex((m) => m.id === messageId);
      if (msgIndex < 0) return;

      const userMessage = messages[msgIndex];
      if (userMessage.role !== "user") return;

      // Remove this message and everything after it
      setMessages((prev) => prev.slice(0, msgIndex));

      // Send the edited message
      await handleSend(newContent);
    },
    [messages, handleSend]
  );

  const handleRewind = useCallback(
    async (messageId: string) => {
      // Find this user message
      const msgIndex = messages.findIndex((m) => m.id === messageId);
      if (msgIndex < 0) return;

      const userMessage = messages[msgIndex];
      if (userMessage.role !== "user") return;

      // Remove this message and everything after it
      setMessages((prev) => prev.slice(0, msgIndex));

      // Re-send the user message
      await handleSend(userMessage.content);
    },
    [messages, handleSend]
  );

  const handleRegenerate = useCallback(
    async (messageId: string) => {
      // Find the user message before this assistant message
      const msgIndex = messages.findIndex((m) => m.id === messageId);
      if (msgIndex <= 0) return;

      const prevMessage = messages[msgIndex - 1];
      if (prevMessage.role !== "user") return;

      // Remove the assistant response and regenerate
      setMessages((prev) => prev.slice(0, msgIndex));

      // Resend the user message (without attachments for now)
      await handleSend(prevMessage.content);
    },
    [messages, handleSend]
  );

  const handleCopy = useCallback(async (content: string) => {
    try {
      if (typeof window !== "undefined" && window.navigator?.clipboard) {
        await window.navigator.clipboard.writeText(content);
      }
    } catch (e) {
      console.error("Failed to copy:", e);
    }
  }, []);

  const handleApprovePermission = async (id: string, _remember?: "once" | "session" | "always") => {
    if (!currentSessionId) return;

    try {
      const req = pendingPermissions.find((p) => p.id === id);
      console.log("[Approve] Tool:", req?.tool, "messageId:", req?.messageId, "args:", req?.args);
      await approveTool(currentSessionId, id, {
        tool: req?.tool,
        args: req?.args,
        messageId: req?.messageId,
      });

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
      const req = pendingPermissions.find((p) => p.id === id);
      await denyTool(currentSessionId, id, {
        tool: req?.tool,
        args: req?.args,
        messageId: req?.messageId,
      });

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

  const handleAnswerQuestion = async (answer: string) => {
    if (!pendingQuestion || !currentSessionId) return;

    try {
      await answerQuestion(currentSessionId, pendingQuestion.question_id, answer);
      setPendingQuestion(null);
    } catch (err) {
      console.error("Failed to answer question:", err);
      setError(`Failed to answer question: ${err}`);
    }
  };

  const needsConnection = sidecarStatus !== "running" && !isConnecting;

  return (
    <div className="flex h-full flex-col">
      {/* Header */}
      <header className="flex items-center justify-between border-b border-border px-6 py-4">
        <div className="flex items-center gap-3">
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
        <div className="flex items-center gap-4">
          {/* Staged operations counter */}
          {usePlanMode && stagedOperations.length > 0 && (
            <motion.div
              initial={{ opacity: 0, scale: 0.8 }}
              animate={{ opacity: 1, scale: 1 }}
              className="flex items-center gap-1.5 px-2.5 py-1 rounded-lg bg-amber-500/10 border border-amber-500/20"
            >
              <div className="h-1.5 w-1.5 rounded-full bg-amber-500 animate-pulse" />
              <span className="text-xs font-medium text-amber-500">
                {stagedOperations.length} change{stagedOperations.length !== 1 ? "s" : ""} pending
              </span>
            </motion.div>
          )}

          <div className="flex items-center gap-2">
            <div
              className={`h-2 w-2 rounded-full ${
                sidecarStatus === "running"
                  ? "bg-primary"
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
        </div>
      </header>

      {/* Plan Mode info banner */}
      {usePlanMode && (
        <motion.div
          initial={{ opacity: 0, height: 0 }}
          animate={{ opacity: 1, height: "auto" }}
          exit={{ opacity: 0, height: 0 }}
          className="bg-primary/5 border-b border-primary/10 px-4 py-3"
        >
          <div className="flex items-start gap-3">
            <div className="mt-0.5 rounded-full bg-primary/10 p-1">
              <AlertCircle className="h-4 w-4 text-primary" />
            </div>
            <div className="flex-1">
              <p className="text-sm font-medium text-text">Plan Mode Active</p>
              <p className="mt-1 text-xs text-text-muted">
                {stagedOperations.length > 0
                  ? `${stagedOperations.length} change${stagedOperations.length !== 1 ? "s" : ""} staged. Review them in the Execution Plan panel (bottom-right) and click "Execute Plan" when ready.`
                  : "The AI will propose file changes for your review. When changes are proposed, they'll appear in the Execution Plan panel for batch approval."}
              </p>
            </div>
          </div>
        </motion.div>
      )}

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
      <div ref={messagesContainerRef} className="flex-1 overflow-y-auto pb-48">
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
          ) : messages.length === 0 && !isGenerating ? (
            <EmptyState
              needsConnection={needsConnection}
              isConnecting={isConnecting}
              onConnect={connectSidecar}
              workspacePath={workspacePath}
              onSendMessage={handleSend}
            />
          ) : (
            messages.map((message, index) => {
              const isLastMessage = index === messages.length - 1;
              const isAssistant = message.role === "assistant";
              const showActionButtons =
                usePlanMode && isLastMessage && isAssistant && !isGenerating;

              // Use content length in key ONLY for streaming messages to force re-renders
              const isActivelyStreaming = isGenerating && isLastMessage && isAssistant;
              const messageKey = isActivelyStreaming
                ? `${message.id}-${message.content?.length || 0}`
                : message.id;

              return (
                <div key={messageKey}>
                  <Message
                    key={messageKey}
                    {...message}
                    isStreaming={isActivelyStreaming}
                    onEdit={handleEdit}
                    onRewind={handleRewind}
                    onRegenerate={handleRegenerate}
                    onCopy={handleCopy}
                    onUndo={isGitRepository ? handleUndo : undefined}
                    onFileOpen={onFileOpen}
                  />
                  {showActionButtons && (
                    <div className="ml-14 mb-4">
                      <PlanActionButtons
                        onImplement={() => {
                          // Switch to immediate mode for execution
                          setUsePlanMode(false);
                          handleSend("Please implement this plan now.");
                        }}
                        onRework={(feedback) => {
                          handleSend(`Please rework the plan with this feedback: ${feedback}

After making the changes, present the updated plan in full (including the complete JSON structure) so I can review it before implementation.`);
                        }}
                        onCancel={() => {
                          clearStaging();
                          handleSend("Let's try a different approach. Cancel the current plan.");
                        }}
                        onViewTasks={onToggleTaskSidebar}
                        disabled={isGenerating}
                        pendingTasks={pendingTasks}
                        onExecuteTasks={() => {
                          // Execute pending tasks with their specific content
                          if (pendingTasks && pendingTasks.length > 0) {
                            console.log(
                              "[ExecuteTasks] Switching to Immediate mode for task execution"
                            );
                            // Switch to immediate mode for execution
                            setUsePlanMode(false);

                            const taskList = pendingTasks
                              .map((t, i) => `${i + 1}. ${t.content}`)
                              .join("\n");
                            const message = `EXECUTION MODE: Please implement the following approved tasks now. Create the files and write the content directly.

${taskList}

Start with task #1 and execute each one. Use the 'write' tool to create files immediately.`;
                            // Force immediate mode for this specific message
                            handleSend(message, undefined, "immediate");
                          }
                        }}
                      />
                    </div>
                  )}
                </div>
              );
            })
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
        selectedAgent={selectedAgent}
        onAgentChange={onAgentChange}
        externalAttachment={fileToAttach}
        onExternalAttachmentProcessed={onFileAttached}
        enabledToolCategories={enabledToolCategories}
        onToolCategoriesChange={setEnabledToolCategories}
      />

      {/* Permission requests - only show in immediate mode */}
      {!usePlanMode && (
        <PermissionToastContainer
          requests={pendingPermissions}
          onApprove={handleApprovePermission}
          onDeny={handleDenyPermission}
        />
      )}

      {/* Question dialog */}
      <QuestionDialog question={pendingQuestion} onAnswer={handleAnswerQuestion} />

      {/* Execution plan panel - only show in plan mode */}
      {usePlanMode && (
        <ExecutionPlanPanel
          operations={stagedOperations}
          onExecute={async () => {
            try {
              await executePlan();
              console.log("[ExecutionPlan] Plan executed successfully");

              // Send confirmation message to AI that plan was executed
              if (currentSessionId && stagedOperations.length > 0) {
                const confirmMessage = `The execution plan with ${stagedOperations.length} change(s) has been applied successfully. You can continue with the next steps.`;

                // Send as a user message so the AI knows to continue
                setTimeout(async () => {
                  try {
                    // Use the same agent (plan agent if in plan mode)
                    await sendMessageStreaming(
                      currentSessionId,
                      confirmMessage,
                      undefined,
                      usePlanMode ? "plan" : undefined
                    );
                  } catch (err) {
                    console.error("[ExecutionPlan] Failed to send confirmation:", err);
                  }
                }, 500); // Small delay to ensure UI updates first
              }
            } catch (err) {
              console.error("[ExecutionPlan] Failed to execute plan:", err);
              setError(`Failed to execute plan: ${err}`);
            }
          }}
          onRemove={async (id) => {
            try {
              await removeOperation(id);
            } catch (err) {
              console.error("[ExecutionPlan] Failed to remove operation:", err);
              setError(`Failed to remove operation: ${err}`);
            }
          }}
          onClear={async () => {
            try {
              await clearStaging();
            } catch (err) {
              console.error("[ExecutionPlan] Failed to clear staging:", err);
              setError(`Failed to clear staging: ${err}`);
            }
          }}
          isExecuting={isExecutingPlan}
        />
      )}

      {/* Activity drawer - Hidden for now as it's always empty */}
      {/* <ActivityDrawer activities={activities} isGenerating={isGenerating} /> */}
    </div>
  );
}

interface EmptyStateProps {
  needsConnection: boolean;
  isConnecting: boolean;
  onConnect: () => void;
  workspacePath: string | null;
  onSendMessage: (message: string) => void;
}

// Suggestion prompts - mix of developer and general user tasks
const SUGGESTION_PROMPTS = [
  {
    title: "📁 Summarize this project",
    description: "Give me an overview of what this project does",
    prompt:
      "Give me a comprehensive overview of this project. What does it do, what are the main components, and how is it organized?",
  },
  {
    title: "🔍 Find and explain",
    description: "Help me understand a specific file or folder",
    prompt: "List the files in this project and help me understand what each one does.",
  },
  {
    title: "📝 Analyze a document",
    description: "Read and summarize any text file",
    prompt:
      "Find any text documents, markdown files, or READMEs in this project and summarize their contents.",
  },
  {
    title: "✨ Suggest improvements",
    description: "What could be better in this project?",
    prompt: "Analyze this project and suggest improvements. What could be done better?",
  },
  {
    title: "🐛 Find issues",
    description: "Look for potential bugs or problems",
    prompt: "Search this codebase for potential bugs, issues, or areas that might cause problems.",
  },
  {
    title: "📖 Create documentation",
    description: "Generate a README or docs",
    prompt:
      "Create comprehensive documentation for this project, including a README with setup instructions.",
  },
];

function EmptyState({
  needsConnection,
  isConnecting,
  onConnect,
  workspacePath,
  onSendMessage,
}: EmptyStateProps) {
  // Randomly select 4 suggestions to show variety
  const [suggestions] = useState(() => {
    const shuffled = [...SUGGESTION_PROMPTS].sort(() => Math.random() - 0.5);
    return shuffled.slice(0, 4);
  });

  return (
    <motion.div
      className="flex min-h-full flex-col items-center justify-center p-8 pt-16"
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
    >
      <div className="max-w-lg w-full text-center">
        <div className="mx-auto mb-6 flex h-16 w-16 items-center justify-center rounded-2xl bg-gradient-to-br from-primary/20 to-secondary/20">
          <Sparkles className="h-8 w-8 text-primary" />
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

        <div className="grid grid-cols-2 gap-3 text-left">
          {suggestions.map((suggestion, index) => (
            <SuggestionCard
              key={index}
              title={suggestion.title}
              description={suggestion.description}
              onClick={() => onSendMessage(suggestion.prompt)}
              disabled={needsConnection || isConnecting}
            />
          ))}
        </div>
      </div>
    </motion.div>
  );
}

interface SuggestionCardProps {
  title: string;
  description: string;
  onClick: () => void;
  disabled?: boolean;
}

function SuggestionCard({ title, description, onClick, disabled }: SuggestionCardProps) {
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className="rounded-lg border border-border bg-surface p-4 text-left transition-all hover:border-primary/50 hover:bg-surface-elevated hover:shadow-lg hover:shadow-primary/5 disabled:opacity-50 disabled:cursor-not-allowed"
    >
      <p className="font-medium text-text">{title}</p>
      <p className="text-sm text-text-muted line-clamp-2">{description}</p>
    </button>
  );
}
