import { useState, useRef, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Message, type MessageProps } from "./Message";
import { ChatInput } from "./ChatInput";
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
  type StreamEvent,
  type SidecarState,
} from "@/lib/tauri";

interface ChatProps {
  workspacePath: string | null;
}

export function Chat({ workspacePath }: ChatProps) {
  const [messages, setMessages] = useState<MessageProps[]>([]);
  const [isGenerating, setIsGenerating] = useState(false);
  const [sessionId, setSessionId] = useState<string | null>(null);
  const [sidecarStatus, setSidecarStatus] = useState<SidecarState>("stopped");
  const [error, setError] = useState<string | null>(null);
  const [isConnecting, setIsConnecting] = useState(false);
  const [pendingPermissions, setPendingPermissions] = useState<PermissionRequest[]>([]);
  const messagesEndRef = useRef<HTMLDivElement>(null);
  const currentAssistantMessageRef = useRef<string>("");

  // Auto-scroll to bottom
  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  // Check sidecar status on mount
  useEffect(() => {
    const checkStatus = async () => {
      try {
        const status = await getSidecarStatus();
        setSidecarStatus(status);
      } catch (e) {
        console.error("Failed to get sidecar status:", e);
      }
    };
    checkStatus();
  }, []);

  const handleStreamEvent = useCallback((event: StreamEvent) => {
    switch (event.type) {
      case "content":
        // Append content to the current assistant message
        currentAssistantMessageRef.current += event.content;
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

      case "thinking":
        // Show thinking content (could be displayed differently)
        currentAssistantMessageRef.current += `\n\n*Thinking: ${event.content}*\n\n`;
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

      case "tool_start": {
        // Add tool call to the message
        setMessages((prev) => {
          const lastMessage = prev[prev.length - 1];
          if (lastMessage && lastMessage.role === "assistant") {
            const toolCalls = lastMessage.toolCalls || [];
            return [
              ...prev.slice(0, -1),
              {
                ...lastMessage,
                toolCalls: [
                  ...toolCalls,
                  {
                    id: event.id,
                    tool: event.tool,
                    args: event.args,
                    status: "pending" as const,
                  },
                ],
              },
            ];
          }
          return prev;
        });

        // Create permission request for destructive operations
        const needsApproval = ["write_file", "create_file", "delete_file", "run_command"].includes(
          event.tool
        );
        if (needsApproval) {
          const permissionRequest: PermissionRequest = {
            id: event.id,
            type: event.tool as PermissionRequest["type"],
            path: event.args.path as string | undefined,
            command: event.args.command as string | undefined,
            reasoning: (event.args.reasoning as string) || "AI wants to perform this action",
            riskLevel:
              event.tool === "delete_file" || event.tool === "run_command" ? "high" : "medium",
          };
          setPendingPermissions((prev) => [...prev, permissionRequest]);
        }
        break;
      }

      case "tool_end":
        // Update tool call with result
        setMessages((prev) => {
          const lastMessage = prev[prev.length - 1];
          if (lastMessage && lastMessage.role === "assistant" && lastMessage.toolCalls) {
            const toolCalls = lastMessage.toolCalls.map((tc) =>
              tc.id === event.id
                ? {
                    ...tc,
                    result: event.error || String(event.result),
                    status: (event.error ? "failed" : "completed") as "failed" | "completed",
                  }
                : tc
            );
            return [...prev.slice(0, -1), { ...lastMessage, toolCalls }];
          }
          return prev;
        });
        break;

      case "done":
        setIsGenerating(false);
        currentAssistantMessageRef.current = "";
        break;

      case "error":
        setError(event.message);
        setIsGenerating(false);
        currentAssistantMessageRef.current = "";
        break;
    }
  }, []);

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

      // Create a new session
      const session = await createSession();
      setSessionId(session.id);
    } catch (e) {
      const errorMessage = e instanceof Error ? e.message : String(e);
      setError(`Failed to start AI: ${errorMessage}`);
      setSidecarStatus("failed");
    } finally {
      setIsConnecting(false);
    }
  };

  const handleSend = async (content: string) => {
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
    let currentSessionId = sessionId;
    if (!currentSessionId) {
      try {
        const session = await createSession();
        currentSessionId = session.id;
        setSessionId(session.id);
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

    try {
      // Send message and stream response
      await sendMessageStreaming(currentSessionId, content);
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
    if (sessionId) {
      try {
        await cancelGeneration(sessionId);
      } catch (e) {
        console.error("Failed to cancel generation:", e);
      }
    }
    setIsGenerating(false);
  };

  const handleApprovePermission = async (id: string, _remember?: "once" | "session" | "always") => {
    if (!sessionId) return;

    try {
      await approveTool(sessionId, id);

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
    if (!sessionId) return;

    try {
      await denyTool(sessionId, id);

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
          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-gradient-to-br from-primary to-secondary">
            <Sparkles className="h-5 w-5 text-white" />
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
      <div className="flex-1 overflow-y-auto">
        <AnimatePresence>
          {messages.length === 0 ? (
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
            className="flex gap-4 bg-surface/50 px-4 py-6"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
          >
            <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-secondary/20">
              <Sparkles className="h-4 w-4 animate-pulse text-secondary" />
            </div>
            <div className="flex items-center gap-2">
              <div className="flex gap-1">
                <span className="h-2 w-2 animate-bounce rounded-full bg-text-subtle [animation-delay:-0.3s]" />
                <span className="h-2 w-2 animate-bounce rounded-full bg-text-subtle [animation-delay:-0.15s]" />
                <span className="h-2 w-2 animate-bounce rounded-full bg-text-subtle" />
              </div>
              <span className="text-sm text-text-muted">Tandem is thinking...</span>
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
        disabled={!workspacePath}
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
