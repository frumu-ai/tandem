import { motion, AnimatePresence } from "framer-motion";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import { cn } from "@/lib/utils";
import {
  User,
  FileText,
  Terminal,
  AlertTriangle,
  Image as ImageIcon,
  Edit3,
  RotateCcw,
  RefreshCw,
  Copy,
  Undo2,
  Check,
  X,
} from "lucide-react";
import { useState } from "react";

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
  // Handlers for message actions
  onEdit?: (messageId: string, newContent: string) => void;
  onRewind?: (messageId: string) => void;
  onRegenerate?: (messageId: string) => void;
  onCopy?: (content: string) => void;
  onUndo?: (messageId: string) => void;
}

export interface ToolCall {
  id: string;
  tool: string;
  args: Record<string, unknown>;
  status: "pending" | "running" | "completed" | "failed";
  result?: string;
}

export function Message({
  id,
  role,
  content,
  timestamp,
  toolCalls,
  isStreaming,
  attachments,
  onEdit,
  onRewind,
  onRegenerate,
  onCopy,
  onUndo,
}: MessageProps) {
  const isUser = role === "user";
  const isSystem = role === "system";
  const [isHovered, setIsHovered] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [editedContent, setEditedContent] = useState(content);
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    onCopy?.(content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleSaveEdit = () => {
    if (editedContent.trim() && editedContent !== content) {
      onEdit?.(id, editedContent);
    }
    setIsEditing(false);
  };

  const handleCancelEdit = () => {
    setEditedContent(content);
    setIsEditing(false);
  };

  return (
    <motion.div
      className={cn(
        "flex gap-4 px-4 py-8 relative group",
        isUser
          ? "bg-transparent border-l-2 border-primary/70"
          : "glass border-glass shadow-lg shadow-black/20 ring-1 ring-white/5"
      )}
      initial={{ opacity: 0, y: 10, filter: "blur(6px)" }}
      animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
      transition={{ duration: 0.25 }}
      onMouseEnter={() => setIsHovered(true)}
      onMouseLeave={() => setIsHovered(false)}
    >
      {/* Avatar */}
      {isUser ? (
        <div className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-lg bg-primary/20 text-primary">
          <User className="h-4 w-4" />
        </div>
      ) : isSystem ? (
        <div className="flex h-8 w-8 flex-shrink-0 items-center justify-center rounded-lg bg-warning/20 text-warning">
          <AlertTriangle className="h-4 w-4" />
        </div>
      ) : (
        <div className="relative flex-shrink-0">
          <div className="absolute inset-0 rounded-xl bg-primary/15 blur-[2px]" />
          <div className="relative h-8 w-8 overflow-hidden rounded-xl ring-1 ring-white/10">
            <img src="/tandem-logo.png" alt="Tandem" className="h-full w-full object-cover" />
          </div>
        </div>
      )}

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
            <span className="flex items-center gap-2 text-xs text-primary font-mono">
              <span className="inline-block h-3 w-1.5 bg-primary animate-pulse" />
              Processing
            </span>
          )}
        </div>

        {/* Hover Action Buttons */}
        <AnimatePresence>
          {isHovered && !isEditing && !isStreaming && (
            <motion.div
              className="absolute top-2 right-2 flex items-center gap-1 glass border-glass rounded-lg px-2 py-1 shadow-lg"
              initial={{ opacity: 0, scale: 0.9, y: -5 }}
              animate={{ opacity: 1, scale: 1, y: 0 }}
              exit={{ opacity: 0, scale: 0.9, y: -5 }}
              transition={{ duration: 0.15 }}
            >
              {isUser && onEdit && (
                <button
                  onClick={() => setIsEditing(true)}
                  className="p-1.5 hover:bg-primary/20 rounded transition-colors text-text-muted hover:text-primary"
                  title="Edit message"
                >
                  <Edit3 className="h-4 w-4" />
                </button>
              )}
              {isUser && onRewind && (
                <button
                  onClick={() => onRewind(id)}
                  className="p-1.5 hover:bg-primary/20 rounded transition-colors text-text-muted hover:text-primary"
                  title="Retry from here"
                >
                  <RotateCcw className="h-4 w-4" />
                </button>
              )}
              {!isUser && onRegenerate && (
                <button
                  onClick={() => onRegenerate(id)}
                  className="p-1.5 hover:bg-primary/20 rounded transition-colors text-text-muted hover:text-primary"
                  title="Regenerate response"
                >
                  <RefreshCw className="h-4 w-4" />
                </button>
              )}
              {!isUser && onCopy && (
                <button
                  onClick={handleCopy}
                  className="p-1.5 hover:bg-primary/20 rounded transition-colors text-text-muted hover:text-primary"
                  title="Copy to clipboard"
                >
                  {copied ? (
                    <Check className="h-4 w-4 text-success" />
                  ) : (
                    <Copy className="h-4 w-4" />
                  )}
                </button>
              )}
              {!isUser && onUndo && (
                <button
                  onClick={() => onUndo(id)}
                  className="p-1.5 hover:bg-warning/20 rounded transition-colors text-text-muted hover:text-warning"
                  title="Undo this message and its file changes"
                >
                  <Undo2 className="h-4 w-4" />
                </button>
              )}
            </motion.div>
          )}
        </AnimatePresence>

        {/* Attachments */}
        {attachments && attachments.length > 0 && (
          <div className="flex flex-wrap gap-2 mb-2">
            {attachments.map((attachment, idx) => (
              <div
                key={idx}
                className="flex items-center gap-2 rounded-lg border-glass bg-surface p-2"
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
        {isEditing ? (
          <div className="space-y-2">
            <textarea
              value={editedContent}
              onChange={(e) => setEditedContent(e.target.value)}
              className="w-full min-h-[100px] p-3 rounded-lg bg-surface border border-glass text-text font-sans resize-y focus:outline-none focus:ring-2 focus:ring-primary/50"
              autoFocus
            />
            <div className="flex gap-2">
              <button
                onClick={handleSaveEdit}
                className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-primary/20 hover:bg-primary/30 text-primary transition-colors"
              >
                <Check className="h-4 w-4" />
                Save & Resend
              </button>
              <button
                onClick={handleCancelEdit}
                className="flex items-center gap-2 px-3 py-1.5 rounded-lg bg-surface hover:bg-surface-elevated text-text-muted transition-colors"
              >
                <X className="h-4 w-4" />
                Cancel
              </button>
            </div>
          </div>
        ) : (
          <div className="prose-custom">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={{
                code({ className, children, ...props }) {
                  const match = /language-(\w+)/.exec(className || "");
                  if (match) {
                    return (
                      <div className="overflow-hidden rounded-lg border border-white/10 bg-surface/60">
                        <div className="flex items-center gap-2 border-b border-white/10 bg-surface-elevated/70 px-3 py-2">
                          <span className="h-2 w-2 rounded-full bg-error/80" />
                          <span className="h-2 w-2 rounded-full bg-warning/80" />
                          <span className="h-2 w-2 rounded-full bg-success/80" />
                          <span className="ml-2 text-[0.65rem] uppercase tracking-widest text-text-subtle terminal-text">
                            code
                          </span>
                        </div>
                        <SyntaxHighlighter
                          style={oneDark}
                          language={match[1]}
                          PreTag="div"
                          customStyle={{ margin: 0, background: "transparent", padding: "1rem" }}
                        >
                          {String(children).replace(/\n$/, "")}
                        </SyntaxHighlighter>
                      </div>
                    );
                  }
                  return (
                    <code className={className} {...props}>
                      {children}
                    </code>
                  );
                },
              }}
            >
              {content}
            </ReactMarkdown>
          </div>
        )}

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
        return "border-glass bg-surface";
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
      className={cn("rounded-lg border p-3 transition-colors", getStatusColor())}
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
          <pre className="font-mono text-xs text-text-subtle">{JSON.stringify(args, null, 2)}</pre>
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
