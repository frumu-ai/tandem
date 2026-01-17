import { motion } from "framer-motion";
import { cn } from "@/lib/utils";
import { User, Bot, FileText, Terminal, AlertTriangle, Image as ImageIcon } from "lucide-react";

export interface MessageAttachment {
  name: string;
  type: "image" | "file";
  preview?: string;
}

export interface MessageProps {
  id: string;
  role: "user" | "assistant" | "system";
  content: string;
  timestamp: Date;
  toolCalls?: ToolCall[];
  isStreaming?: boolean;
  attachments?: MessageAttachment[];
}

export interface ToolCall {
  id: string;
  tool: string;
  args: Record<string, unknown>;
  status: "pending" | "running" | "completed" | "failed";
  result?: string;
}

export function Message({ role, content, timestamp, toolCalls, isStreaming, attachments }: MessageProps) {
  const isUser = role === "user";
  const isSystem = role === "system";

  return (
    <motion.div
      className={cn(
        "flex gap-4 px-4 py-6",
        isUser ? "bg-transparent" : "bg-surface/50"
      )}
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.2 }}
    >
      {/* Avatar */}
      <div
        className={cn(
          "flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-lg",
          isUser
            ? "bg-primary/20 text-primary"
            : isSystem
            ? "bg-warning/20 text-warning"
            : "bg-secondary/20 text-secondary"
        )}
      >
        {isUser ? (
          <User className="h-4 w-4" />
        ) : isSystem ? (
          <AlertTriangle className="h-4 w-4" />
        ) : (
          <Bot className="h-4 w-4" />
        )}
      </div>

      {/* Content */}
      <div className="flex-1 space-y-3">
        <div className="flex items-center gap-2">
          <span className="font-medium text-text">
            {isUser ? "You" : isSystem ? "System" : "Tandem"}
          </span>
          <span className="text-xs text-text-subtle">
            {timestamp.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}
          </span>
          {isStreaming && (
            <span className="flex items-center gap-1 text-xs text-primary">
              <span className="h-1.5 w-1.5 animate-pulse rounded-full bg-primary" />
              Thinking...
            </span>
          )}
        </div>

        {/* Attachments */}
        {attachments && attachments.length > 0 && (
          <div className="flex flex-wrap gap-2 mb-2">
            {attachments.map((attachment, idx) => (
              <div
                key={idx}
                className="flex items-center gap-2 rounded-lg border border-border bg-surface p-2"
              >
                {attachment.type === "image" && attachment.preview ? (
                  <img
                    src={attachment.preview}
                    alt={attachment.name}
                    className="h-12 w-12 rounded object-cover"
                  />
                ) : attachment.type === "image" ? (
                  <ImageIcon className="h-6 w-6 text-text-muted" />
                ) : (
                  <FileText className="h-6 w-6 text-text-muted" />
                )}
                <span className="text-xs text-text-muted max-w-[100px] truncate">
                  {attachment.name}
                </span>
              </div>
            ))}
          </div>
        )}

        {/* Message content */}
        <div className="prose prose-invert max-w-none">
          <p className="whitespace-pre-wrap text-text-muted">{content}</p>
        </div>

        {/* Tool calls */}
        {toolCalls && toolCalls.length > 0 && (
          <div className="space-y-2">
            {toolCalls.map((tool) => (
              <ToolCallCard key={tool.id} {...tool} />
            ))}
          </div>
        )}
      </div>
    </motion.div>
  );
}

function ToolCallCard({ tool, args, status, result }: ToolCall) {
  const getIcon = () => {
    switch (tool) {
      case "read_file":
      case "write_file":
        return <FileText className="h-4 w-4" />;
      case "run_command":
        return <Terminal className="h-4 w-4" />;
      default:
        return <FileText className="h-4 w-4" />;
    }
  };

  const getStatusColor = () => {
    switch (status) {
      case "pending":
        return "border-border bg-surface";
      case "running":
        return "border-primary/50 bg-primary/10";
      case "completed":
        return "border-success/50 bg-success/10";
      case "failed":
        return "border-error/50 bg-error/10";
    }
  };

  return (
    <motion.div
      className={cn(
        "rounded-lg border p-3 transition-colors",
        getStatusColor()
      )}
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
    >
      <div className="flex items-center gap-2">
        <div className="text-text-muted">{getIcon()}</div>
        <span className="font-mono text-sm text-text">{tool}</span>
        {status === "running" && (
          <div className="ml-auto h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
        )}
      </div>
      
      {args && Object.keys(args).length > 0 && (
        <div className="mt-2 rounded bg-surface p-2">
          <pre className="font-mono text-xs text-text-subtle">
            {JSON.stringify(args, null, 2)}
          </pre>
        </div>
      )}

      {result && (
        <div className="mt-2 rounded bg-surface p-2">
          <pre className="font-mono text-xs text-text-muted">{result}</pre>
        </div>
      )}
    </motion.div>
  );
}
