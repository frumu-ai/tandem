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
  ExternalLink,
  ChevronDown,
  ChevronUp,
  Brain,
} from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import React, {
  startTransition,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

export interface MessageAttachment {
  name: string;
  type: "image" | "file";
  preview?: string;
}

const MAX_CODE_LINES = 25;
const NEAR_VIEWPORT_ROOT_MARGIN = "900px 0px";
const codeHighlightReadyCache = new Set<string>();
const codeExpandedCache = new Set<string>();

function stableStringHash(input: string): string {
  let hash = 5381;
  for (let i = 0; i < input.length; i += 1) {
    hash = (hash * 33) ^ input.charCodeAt(i);
  }
  return (hash >>> 0).toString(36);
}

function requestIdle(work: () => void): () => void {
  if (typeof globalThis.requestIdleCallback === "function") {
    const id = globalThis.requestIdleCallback(() => work(), { timeout: 300 });
    return () => {
      if (typeof globalThis.cancelIdleCallback === "function") {
        globalThis.cancelIdleCallback(id);
      }
    };
  }
  const timeout = globalThis.setTimeout(work, 32);
  return () => globalThis.clearTimeout(timeout);
}

function useNearViewport(
  ref: React.RefObject<unknown>,
  options?: { rootMargin?: string }
): boolean {
  const [isNear, setIsNear] = useState(false);

  useEffect(() => {
    const target = ref.current;
    if (!target) return;

    const observer = new globalThis.IntersectionObserver(
      (entries) => {
        const entry = entries[0];
        if (!entry) return;
        if (entry.isIntersecting) {
          setIsNear(true);
          observer.disconnect();
        }
      },
      { rootMargin: options?.rootMargin ?? "0px" }
    );

    (observer.observe as (node: unknown) => void)(target);
    return () => observer.disconnect();
  }, [options?.rootMargin, ref]);

  return isNear;
}

const CollapsibleCodeBlock = ({
  language,
  children,
  renderMode,
  ...props
}: {
  language: string;
  children: React.ReactNode;
  renderMode: "full" | "streaming-lite";
  [key: string]: unknown;
}) => {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const codeString = String(children).replace(/\n$/, "");
  const codeLines = useMemo(() => codeString.split("\n"), [codeString]);
  const lineCount = codeLines.length;
  const isLong = lineCount > MAX_CODE_LINES;
  const codeHash = useMemo(() => stableStringHash(codeString), [codeString]);
  const cacheKey = useMemo(() => `${language}:${codeHash}`, [codeHash, language]);
  const [isExpanded, setIsExpanded] = useState(() => codeExpandedCache.has(cacheKey));
  const nearViewport = useNearViewport(containerRef, {
    rootMargin: NEAR_VIEWPORT_ROOT_MARGIN,
  });
  const shouldDeferDuringStreaming = renderMode === "streaming-lite" && !isExpanded;
  const [isHighlightReady, setIsHighlightReady] = useState(
    () =>
      codeHighlightReadyCache.has(cacheKey) ||
      codeExpandedCache.has(cacheKey) ||
      (!isLong && !shouldDeferDuringStreaming)
  );
  const showCollapsedPreview = isLong && !isExpanded;
  const highlightReady = !showCollapsedPreview && (!shouldDeferDuringStreaming || isHighlightReady);

  useEffect(() => {
    if (shouldDeferDuringStreaming && nearViewport && !isHighlightReady) {
      return requestIdle(() => {
        startTransition(() => {
          codeHighlightReadyCache.add(cacheKey);
          setIsHighlightReady(true);
        });
      });
    }
  }, [cacheKey, isHighlightReady, nearViewport, shouldDeferDuringStreaming]);

  useEffect(() => {
    if (isExpanded) {
      codeExpandedCache.add(cacheKey);
      codeHighlightReadyCache.add(cacheKey);
    } else {
      codeExpandedCache.delete(cacheKey);
    }
  }, [cacheKey, isExpanded]);

  const previewCode = useMemo(() => {
    if (isExpanded || !isLong) return codeString;
    return codeLines.slice(0, MAX_CODE_LINES).join("\n");
  }, [codeLines, codeString, isExpanded, isLong]);

  return (
    <div
      ref={containerRef}
      className="group overflow-hidden rounded-lg border border-white/10 bg-surface/60 code-block-shell"
    >
      <div className="flex items-center justify-between border-b border-white/10 bg-surface-elevated/70 px-3 py-2">
        <div className="flex items-center gap-2">
          <span className="h-2 w-2 rounded-full bg-error/80" />
          <span className="h-2 w-2 rounded-full bg-warning/80" />
          <span className="h-2 w-2 rounded-full bg-success/80" />
          <span className="ml-2 text-[0.65rem] uppercase tracking-widest text-text-subtle terminal-text">
            {language || "text"}
          </span>
        </div>
      </div>

      <div className={cn("relative", !isExpanded && isLong && "max-h-[320px] overflow-hidden")}>
        {highlightReady ? (
          <SyntaxHighlighter
            style={oneDark}
            language={language}
            PreTag="div"
            customStyle={{ margin: 0, background: "transparent", padding: "1rem" }}
            {...props}
          >
            {codeString}
          </SyntaxHighlighter>
        ) : (
          <pre className="m-0 overflow-auto p-4 font-mono text-xs leading-relaxed text-text">
            <code>{previewCode}</code>
          </pre>
        )}

        {!isExpanded && isLong && (
          <div className="absolute inset-x-0 bottom-0 flex items-end justify-center bg-gradient-to-t from-surface-elevated via-surface-elevated/90 to-transparent pb-4 pt-16">
            <button
              onClick={() => {
                setIsExpanded(true);
                codeExpandedCache.add(cacheKey);
                codeHighlightReadyCache.add(cacheKey);
                setIsHighlightReady(true);
              }}
              className="flex items-center gap-2 rounded-full border border-primary/20 bg-primary/10 px-4 py-1.5 text-xs font-medium text-primary shadow-lg shadow-black/20 transition-all hover:border-primary/40 hover:bg-primary/20 backdrop-blur-sm"
            >
              Show {lineCount - MAX_CODE_LINES} more lines
              <ChevronDown className="h-3 w-3" />
            </button>
          </div>
        )}
      </div>

      {isExpanded && isLong && (
        <div className="flex justify-center border-t border-white/5 bg-surface-elevated/30 py-2">
          <button
            onClick={() => {
              setIsExpanded(false);
              codeExpandedCache.delete(cacheKey);
            }}
            className="flex items-center gap-2 rounded px-3 py-1 text-xs text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
          >
            Collapse
            <ChevronUp className="h-3 w-3" />
          </button>
        </div>
      )}
    </div>
  );
};

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
  onFileOpen?: (filePath: string) => void;
  onOpenQuestionToolCall?: (args: { messageId: string; toolCallId: string }) => void;
  isQuestionToolCallPending?: (args: { messageId: string; toolCallId: string }) => boolean;
  memoryRetrieval?: {
    status?:
      | "not_attempted"
      | "attempted_no_hits"
      | "retrieved_used"
      | "degraded_disabled"
      | "error_fallback";
    used: boolean;
    chunks_total: number;
    latency_ms: number;
    embedding_status?: string;
    embedding_reason?: string;
    session_chunks_stored?: number;
    project_chunks_stored?: number;
  } | null;
  renderMode?: "full" | "streaming-lite";
  disableMountAnimation?: boolean;
}

export interface ToolCall {
  id: string;
  tool: string;
  args: Record<string, unknown>;
  status: "pending" | "running" | "completed" | "failed";
  result?: string;
  isTechnical?: boolean;
}

const FILE_EXTENSIONS =
  "json|ts|tsx|js|jsx|md|txt|py|rs|go|java|cpp|c|h|css|scss|html|xml|yaml|yml|toml|pptx|ppt|docx|doc|xlsx|xls|pdf|png|jpg|jpeg|gif|svg|webp";
const FILE_PATH_BASE =
  String.raw`@?(?:` +
  String.raw`(?:[A-Za-z]:[\\/]|\.{1,2}[\\/]|\/)[\w\-./\\]+` +
  String.raw`|` +
  String.raw`[\w\-.]+[\\/][\w\-./\\]+` +
  String.raw`)` +
  String.raw`\.` + // MUST have a literal dot
  `(?:${FILE_EXTENSIONS})`;
const FILE_PATH_EXACT = new RegExp(`^${FILE_PATH_BASE}$`, "i");

const normalizeFilePath = (rawPath: string) =>
  rawPath.startsWith("@") ? rawPath.slice(1) : rawPath;

/**
 * FilePathParser - Detects file paths in text and renders them as clickable links
 * Matches common file path patterns:
 * - Windows: C:\path\to\file.ext or c:/path/to/file.ext
 * - Unix: /path/to/file.ext or ./relative/path.ext or ../relative/path.ext
 * - Relative: path/to/file.ext
 * - Optional @ prefix for context mentions
 * - Extensions: .json, .ts, .tsx, .js, .jsx, .md, .txt, .pptx, etc.
 */
function FilePathParser({
  text,
  onFileOpen,
}: {
  text: string;
  onFileOpen?: (filePath: string) => void;
}) {
  const filePathRegex = new RegExp(`${FILE_PATH_BASE}\\b`, "gi");

  const parts: (string | ReactNode)[] = [];
  let lastIndex = 0;
  let match;

  while ((match = filePathRegex.exec(text)) !== null) {
    const rawPath = match[0]; // Full matched path (may include @)
    const filePath = normalizeFilePath(rawPath);
    const matchStart = match.index;

    // Add text before the match
    if (matchStart > lastIndex) {
      parts.push(text.substring(lastIndex, matchStart));
    }

    // Add clickable file link
    parts.push(
      <button
        key={`file-${matchStart}`}
        onClick={() => onFileOpen?.(filePath)}
        className="inline-flex items-center gap-1 text-primary hover:text-primary/80 hover:underline transition-colors font-mono text-sm"
        title={`Open ${filePath}`}
        type="button"
      >
        <ExternalLink className="h-3 w-3" />
        {rawPath}
      </button>
    );

    lastIndex = matchStart + rawPath.length;
  }

  // Add remaining text
  if (lastIndex < text.length) {
    parts.push(text.substring(lastIndex));
  }

  return <>{parts.length > 0 ? parts : text}</>;
}

export const Message = React.memo(MessageComponent);

function MessageComponent({
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
  onFileOpen,
  onOpenQuestionToolCall,
  isQuestionToolCallPending,
  memoryRetrieval,
  renderMode = "full",
  disableMountAnimation = false,
}: MessageProps) {
  const isUser = role === "user";
  const isSystem = role === "system";
  const [isHovered, setIsHovered] = useState(false);
  const [isEditing, setIsEditing] = useState(false);
  const [editedContent, setEditedContent] = useState(content);
  const [copied, setCopied] = useState(false);

  const handleExternalLinkOpen = useCallback(async (href: string) => {
    if (!href) return;
    try {
      await openUrl(href);
      return;
    } catch (error) {
      console.warn("[Message] Failed to open external URL via Tauri opener:", error);
    }

    // Browser fallback for web/non-tauri environments.
    try {
      globalThis.open(href, "_blank", "noopener,noreferrer");
    } catch (error) {
      console.error("[Message] Failed to open external URL fallback:", error);
    }
  }, []);

  // Memoize markdown components to prevent re-creation on every render
  const markdownComponents = React.useMemo(
    () => ({
      a({
        href,
        children,
        ...props
      }: React.ComponentPropsWithoutRef<"a"> & { children?: ReactNode }) {
        // Treat markdown links to local files as "open in file browser"
        // instead of navigating the SPA (which refreshes the app).
        if (href && FILE_PATH_EXACT.test(href)) {
          const normalizedHref = normalizeFilePath(href);
          return (
            <button
              onClick={() => onFileOpen?.(normalizedHref)}
              className="inline-flex items-center gap-1 text-primary hover:text-primary/80 hover:underline transition-colors font-mono text-sm"
              title={`Open ${normalizedHref}`}
              type="button"
            >
              <ExternalLink className="h-3 w-3" />
              {children}
            </button>
          );
        }

        // Normal URL: keep as link (open in new tab/window).
        return (
          <a
            href={href}
            onClick={(event) => {
              if (!href) return;
              event.preventDefault();
              void handleExternalLinkOpen(href);
            }}
            className="text-primary hover:underline"
            {...props}
          >
            {children}
          </a>
        );
      },
      code({
        className,
        children,
        inline,
        ...props
      }: {
        className?: string;
        children?: ReactNode;
        inline?: boolean;
      } & React.HTMLAttributes<HTMLElement>) {
        const match = /language-(\w+)/.exec(className || "");
        if (match) {
          return (
            <CollapsibleCodeBlock language={match[1]} renderMode={renderMode} {...props}>
              {children}
            </CollapsibleCodeBlock>
          );
        }
        const inlineText = String(children).trim();
        if (inline && FILE_PATH_EXACT.test(inlineText)) {
          const normalizedInline = normalizeFilePath(inlineText);
          return (
            <button
              onClick={() => onFileOpen?.(normalizedInline)}
              className="inline-flex items-center gap-1 text-primary hover:text-primary/80 hover:underline transition-colors font-mono text-sm"
              title={`Open ${normalizedInline}`}
              type="button"
            >
              <ExternalLink className="h-3 w-3" />
              {inlineText}
            </button>
          );
        }
        return (
          <code className={className} {...props}>
            {children}
          </code>
        );
      },
      // Custom paragraph renderer to parse file paths in text
      p: ({ children }: { children?: ReactNode }) => (
        <p>
          {React.Children.map(children, (child, index) =>
            typeof child === "string" ? (
              <FilePathParser key={index} text={child} onFileOpen={onFileOpen} />
            ) : (
              child
            )
          )}
        </p>
      ),
      // Custom list item renderer to parse file paths
      li: ({ children }: { children?: ReactNode }) => (
        <li>
          {React.Children.map(children, (child, index) =>
            typeof child === "string" ? (
              <FilePathParser key={index} text={child} onFileOpen={onFileOpen} />
            ) : (
              child
            )
          )}
        </li>
      ),
    }),
    [handleExternalLinkOpen, onFileOpen, renderMode]
  );

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

  const containerClassName = cn(
    "flex gap-4 px-4 py-8 relative group",
    isUser
      ? "bg-transparent border-l-2 border-primary/70"
      : "glass border-glass shadow-lg shadow-black/20 ring-1 ring-white/5"
  );

  const containerHandlers = {
    onMouseEnter: () => setIsHovered(true),
    onMouseLeave: () => setIsHovered(false),
  };

  const messageBody = (
    <>
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
      <div className="flex-1 min-w-0 space-y-3">
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
          {!isUser && !isSystem && memoryRetrieval && (
            <>
              <span
                className={cn(
                  "inline-flex items-center gap-1 text-[11px] rounded-md px-2 py-0.5 border",
                  memoryRetrieval.used
                    ? "bg-primary/15 border-primary/40 text-primary"
                    : "bg-warning/15 border-warning/40 text-warning"
                )}
              >
                <Brain className="h-3 w-3" />
                {memoryRetrieval.status === "retrieved_used" || memoryRetrieval.used
                  ? `Memory: ${memoryRetrieval.chunks_total} chunks (${memoryRetrieval.latency_ms}ms)`
                  : memoryRetrieval.status === "error_fallback"
                    ? "Memory: fallback"
                    : memoryRetrieval.status === "attempted_no_hits"
                      ? "Memory: no hits"
                      : memoryRetrieval.status === "degraded_disabled"
                        ? "Memory: degraded"
                        : "Memory: not attempted"}
              </span>
              {memoryRetrieval.embedding_status && (
                <span
                  title={memoryRetrieval.embedding_reason}
                  className={cn(
                    "inline-flex items-center gap-1 text-[11px] rounded-md px-2 py-0.5 border",
                    memoryRetrieval.embedding_status === "ok"
                      ? "bg-success/15 border-success/40 text-success"
                      : "bg-warning/15 border-warning/40 text-warning"
                  )}
                >
                  Embeddings: {memoryRetrieval.embedding_status}
                </span>
              )}
              {(memoryRetrieval.session_chunks_stored || memoryRetrieval.project_chunks_stored) && (
                <span className="inline-flex items-center gap-1 text-[11px] rounded-md px-2 py-0.5 border bg-sky-500/10 border-sky-500/30 text-sky-300">
                  Store: S{memoryRetrieval.session_chunks_stored ?? 0} / P
                  {memoryRetrieval.project_chunks_stored ?? 0}
                </span>
              )}
            </>
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
              <div key={idx} className="flex items-center gap-2 rounded-lg glass border-glass p-2">
                {attachment.type === "image" && attachment.preview && attachment.preview !== "" ? (
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
            <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
              {content}
            </ReactMarkdown>
          </div>
        )}

        {/* Tool calls - collapsed when multiple */}
        {toolCalls &&
          toolCalls.length > 0 &&
          (toolCalls.length >= 2 ? (
            <CollapsedToolCalls
              toolCalls={toolCalls}
              parentMessageId={id}
              messageTimestamp={timestamp}
              onOpenQuestionToolCall={onOpenQuestionToolCall}
              isQuestionToolCallPending={isQuestionToolCallPending}
            />
          ) : (
            <div className="space-y-2">
              {toolCalls.map((tool) => (
                <ToolCallCard
                  key={tool.id}
                  {...tool}
                  parentMessageId={id}
                  onOpenQuestionToolCall={onOpenQuestionToolCall}
                  isQuestionToolCallPending={isQuestionToolCallPending}
                />
              ))}
            </div>
          ))}
      </div>
    </>
  );

  return disableMountAnimation ? (
    <div className={containerClassName} {...containerHandlers}>
      {messageBody}
    </div>
  ) : (
    <motion.div
      className={containerClassName}
      initial={{ opacity: 0, y: 10 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.15 }}
      {...containerHandlers}
    >
      {messageBody}
    </motion.div>
  );
}

// Collapsed tool calls for Plan Mode - shows summary with expand toggle
const CollapsedToolCalls = React.memo(function CollapsedToolCalls({
  toolCalls,
  parentMessageId,
  messageTimestamp,
  onOpenQuestionToolCall,
  isQuestionToolCallPending,
}: {
  toolCalls: ToolCall[];
  parentMessageId: string;
  messageTimestamp: Date;
  onOpenQuestionToolCall?: (args: { messageId: string; toolCallId: string }) => void;
  isQuestionToolCallPending?: (args: { messageId: string; toolCallId: string }) => boolean;
}) {
  const [isExpanded, setIsExpanded] = useState(false);
  const [nowMs, setNowMs] = useState<number>(() => new Date().getTime());
  const [endedAtMs, setEndedAtMs] = useState<number | null>(null);

  const runningCount = useMemo(
    () => toolCalls.filter((t) => t.status === "running").length,
    [toolCalls]
  );
  const completedCount = useMemo(
    () => toolCalls.filter((t) => t.status === "completed").length,
    [toolCalls]
  );
  const pendingCount = useMemo(
    () => toolCalls.filter((t) => t.status === "pending").length,
    [toolCalls]
  );
  const failedCount = useMemo(
    () => toolCalls.filter((t) => t.status === "failed").length,
    [toolCalls]
  );
  const isTerminal = runningCount === 0 && pendingCount === 0 && toolCalls.length > 0;

  useEffect(() => {
    if (isTerminal) return;
    const id = globalThis.setInterval(() => {
      setNowMs(new Date().getTime());
    }, 1000);
    return () => globalThis.clearInterval(id);
  }, [isTerminal]);

  useEffect(() => {
    if (isTerminal && endedAtMs == null) {
      setEndedAtMs(new Date().getTime());
      return;
    }
    if (!isTerminal && endedAtMs != null) {
      setEndedAtMs(null);
    }
  }, [isTerminal, endedAtMs]);

  const durationSec = Math.max(
    1,
    Math.round(((endedAtMs ?? nowMs) - messageTimestamp.getTime()) / 1000)
  );

  // Group by tool type for summary
  const summary = useMemo(() => {
    const toolGroups = toolCalls.reduce(
      (acc, tool) => {
        const name = tool.tool.replace(/^(read_file|write_file|read|write)$/, (m) =>
          m.includes("read") ? "read" : "write"
        );
        acc[name] = (acc[name] || 0) + 1;
        return acc;
      },
      {} as Record<string, number>
    );

    return Object.entries(toolGroups)
      .map(([name, count]) => `${count} ${name}`)
      .join(", ");
  }, [toolCalls]);

  return (
    <motion.div
      className="rounded-lg border border-glass bg-surface/50 overflow-hidden"
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
    >
      <button
        onClick={() => setIsExpanded(!isExpanded)}
        className="w-full flex items-center gap-3 px-3 py-2 hover:bg-surface-elevated/50 transition-colors"
      >
        <div className="flex items-center gap-2 text-text-muted">
          <Terminal className="h-4 w-4" />
          <span className="rounded border border-border bg-surface px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-text-subtle">
            {runningCount > 0 ? "running" : failedCount > 0 ? "issues" : "complete"}
          </span>
          <span className="text-xs font-medium">
            {toolCalls.length} step{toolCalls.length === 1 ? "" : "s"}
          </span>
        </div>

        <span className="text-xs text-text-subtle truncate flex-1 text-left">{summary}</span>

        {runningCount > 0 && (
          <div className="flex items-center gap-1 text-primary">
            <div className="h-3 w-3 animate-spin rounded-full border-2 border-primary border-t-transparent" />
            <span className="text-xs">{runningCount} running</span>
          </div>
        )}

        {pendingCount > 0 && <span className="text-xs text-warning">{pendingCount} pending</span>}
        {failedCount > 0 && <span className="text-xs text-error">{failedCount} failed</span>}
        <span className="text-xs text-text-subtle">{durationSec}s</span>

        {completedCount === toolCalls.length && (
          <span className="text-xs text-success">✓ all complete</span>
        )}

        <ChevronDown
          className={cn("h-4 w-4 text-text-muted transition-transform", isExpanded && "rotate-180")}
        />
      </button>

      <AnimatePresence>
        {isExpanded && (
          <motion.div
            initial={{ height: 0 }}
            animate={{ height: "auto" }}
            exit={{ height: 0 }}
            className="overflow-hidden"
          >
            <div className="p-2 space-y-2 border-t border-glass">
              {toolCalls.map((tool) => (
                <ToolCallCard
                  key={tool.id}
                  {...tool}
                  parentMessageId={parentMessageId}
                  onOpenQuestionToolCall={onOpenQuestionToolCall}
                  isQuestionToolCallPending={isQuestionToolCallPending}
                />
              ))}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  );
});

const ToolCallCard = React.memo(function ToolCallCard({
  id,
  tool,
  args,
  status,
  result,
  parentMessageId,
  onOpenQuestionToolCall,
  isQuestionToolCallPending,
}: ToolCall & {
  parentMessageId: string;
  onOpenQuestionToolCall?: (args: { messageId: string; toolCallId: string }) => void;
  isQuestionToolCallPending?: (args: { messageId: string; toolCallId: string }) => boolean;
}) {
  const [isExpanded, setIsExpanded] = useState(false);

  // Don't render completed technical tools (redundant if Chat.tsx filters them,
  // but good for safety)
  // if (isTechnical && status === "completed") return null; // ALWAYS SHOW TOOLS PER USER REQUEST

  const getIcon = () => {
    switch (tool) {
      case "read_file":
      case "write_file":
      case "todowrite":
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

  // Helper to summarizing args for specific tools
  const serializedArgs = useMemo(
    () => (args && Object.keys(args).length > 0 ? JSON.stringify(args, null, 2) : ""),
    [args]
  );

  const getArgsSummary = () => {
    if (!args) return null;

    // For tools that take file content, just show the filename and size
    if (args.TargetFile || args.file_path || args.path) {
      const filename = (args.TargetFile || args.file_path || args.path) as string;
      const content = (args.CodeContent || args.content || args.text) as string;
      if (content) {
        return (
          <div className="flex items-center gap-2 text-xs text-text-subtle">
            <span className="font-medium">{filename}</span>
            <span className="text-text-muted">({content.length} chars)</span>
          </div>
        );
      }
      return <div className="text-xs text-text-subtle">{filename}</div>;
    }

    // Default: truncated JSON
    const json = serializedArgs;
    if (json.length > 100) {
      return <div className="text-xs text-text-subtle">{json.substring(0, 100)}...</div>;
    }
    return (
      <pre className="font-mono text-xs text-text-subtle whitespace-pre-wrap break-words">
        {json}
      </pre>
    );
  };

  const hasContent = (args && Object.keys(args).length > 0) || result;
  const isQuestionTool = tool === "question";
  const showAnswerQuestion =
    isQuestionTool &&
    (isQuestionToolCallPending
      ? isQuestionToolCallPending({ messageId: parentMessageId, toolCallId: id })
      : true);

  return (
    <motion.div
      className={cn(
        "rounded-lg border p-3 transition-colors w-full max-w-full min-w-0 overflow-hidden",
        getStatusColor()
      )}
      initial={{ opacity: 0, scale: 0.95 }}
      animate={{ opacity: 1, scale: 1 }}
    >
      <div
        className={cn("flex items-center gap-2", hasContent && "cursor-pointer")}
        onClick={() => hasContent && setIsExpanded(!isExpanded)}
      >
        <div className="text-text-muted">{getIcon()}</div>
        <span className="font-mono text-sm text-text">{tool}</span>
        {status === "running" && (
          <div className="ml-auto h-4 w-4 animate-spin rounded-full border-2 border-primary border-t-transparent" />
        )}
        {hasContent && (
          <ChevronDown
            className={cn(
              "ml-auto h-4 w-4 text-text-muted transition-transform",
              isExpanded && "rotate-180"
            )}
          />
        )}
      </div>

      <AnimatePresence>
        {isExpanded && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="overflow-hidden"
          >
            {args && Object.keys(args).length > 0 && (
              <div className="mt-2 rounded bg-surface p-2 overflow-x-auto max-w-full">
                <p className="mb-1 text-[10px] uppercase font-bold text-text-muted">Arguments</p>
                <pre className="font-mono text-xs text-text-subtle whitespace-pre-wrap break-words">
                  {serializedArgs}
                </pre>
              </div>
            )}

            {result && (
              <div className="mt-2 rounded bg-surface p-2 overflow-x-auto max-w-full">
                <p className="mb-1 text-[10px] uppercase font-bold text-text-muted">Result</p>
                <pre className="font-mono text-xs text-text-muted whitespace-pre-wrap break-words">
                  {result}
                </pre>
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {!isExpanded && hasContent && <div className="mt-2 pl-6">{getArgsSummary()}</div>}

      {showAnswerQuestion && (
        <div className="mt-3 flex justify-end">
          <button
            type="button"
            className="text-xs text-primary hover:text-primary/80 hover:underline"
            onClick={(e) => {
              e.stopPropagation();
              onOpenQuestionToolCall?.({ messageId: parentMessageId, toolCallId: id });
            }}
          >
            Answer this question
          </button>
        </div>
      )}
    </motion.div>
  );
});
