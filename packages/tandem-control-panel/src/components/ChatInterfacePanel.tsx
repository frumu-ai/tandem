import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { useEffect, useRef } from "react";
import type { ReactNode } from "react";
import { renderMarkdownSafe } from "../lib/markdown";
import { Icon, Spinner } from "../ui";

export type ChatInterfaceMessage = {
  id: string;
  role: string;
  displayRole?: string;
  text: string;
  markdown?: boolean;
};

export type ChatQuickReply = {
  id: string;
  label: string;
};

export type BotIdentity = {
  botName?: string;
  botAvatarUrl?: string;
};

type ChatInterfacePanelProps = {
  messages: ChatInterfaceMessage[];
  emptyText: string;
  inputValue: string;
  inputPlaceholder: string;
  sendLabel: string;
  onInputChange: (value: string) => void;
  onSend: () => void;
  sendDisabled?: boolean;
  inputDisabled?: boolean;
  statusTitle?: string;
  statusDetail?: string;
  questionTitle?: string;
  questionText?: string;
  quickReplies?: ChatQuickReply[];
  onQuickReply?: (option: ChatQuickReply) => void;
  questionHint?: string;
  botIdentity?: BotIdentity;
  streamingText?: string;
  showThinking?: boolean;
  thinkingText?: string;
  autoFocusKey?: string | number;
  attachments?: Array<{ path: string; name?: string; size?: number }>;
  onOpenAttachment?: (index: number) => void;
  onRemoveAttachment?: (index: number) => void;
  onAttach?: () => void;
  attachDisabled?: boolean;
  composerAccessory?: ReactNode;
  className?: string;
};

export function ChatInterfacePanel({
  messages,
  emptyText,
  inputValue,
  inputPlaceholder,
  sendLabel,
  onInputChange,
  onSend,
  sendDisabled = false,
  inputDisabled = false,
  statusTitle = "",
  statusDetail = "",
  questionTitle = "",
  questionText = "",
  quickReplies = [],
  onQuickReply,
  questionHint = "",
  botIdentity,
  streamingText = "",
  showThinking = false,
  thinkingText = "Thinking",
  autoFocusKey,
  attachments = [],
  onOpenAttachment,
  onRemoveAttachment,
  onAttach,
  attachDisabled = false,
  composerAccessory,
  className = "",
}: ChatInterfacePanelProps) {
  const reducedMotion = !!useReducedMotion();
  const panelRef = useRef<HTMLDivElement>(null);
  const messagesRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const stickToBottomRef = useRef(true);

  const isNearBottom = (element: HTMLDivElement) => {
    const distance = element.scrollHeight - element.scrollTop - element.clientHeight;
    return distance <= 24;
  };

  useEffect(() => {
    const host = messagesRef.current;
    if (!host) return;
    if (!stickToBottomRef.current) return;
    host.scrollTop = host.scrollHeight;
  }, [messages, streamingText, showThinking]);

  useEffect(() => {
    const area = inputRef.current;
    if (!area) return;
    area.style.height = "0px";
    area.style.height = `${Math.min(area.scrollHeight, 180)}px`;
  }, [inputValue]);

  useEffect(() => {
    if (autoFocusKey == null || inputDisabled) return;
    const area = inputRef.current;
    if (!area) return;
    area.focus();
    try {
      const len = area.value.length;
      area.setSelectionRange(len, len);
    } catch {
      // ignore selection failures on older browsers
    }
  }, [autoFocusKey, inputDisabled]);

  const assistantLabel = botIdentity?.botName || "Assistant";

  return (
    <div
      ref={panelRef}
      className={`rounded-2xl border border-white/10 bg-black/20 p-3 flex min-h-0 flex-1 flex-col ${className}`}
    >
      {questionText ? (
        <div className="mb-3 rounded-xl border border-sky-500/40 bg-sky-950/30 p-3">
          <div className="text-xs uppercase tracking-wide text-sky-200">
            {questionTitle || "Planner question"}
          </div>
          <div className="mt-1 text-sm font-medium text-sky-100">{questionText}</div>
          {quickReplies.length ? (
            <div className="mt-2 flex flex-wrap gap-2">
              {quickReplies.map((option) => (
                <button
                  key={option.id}
                  type="button"
                  className="tcp-btn text-sm"
                  disabled={inputDisabled}
                  onClick={() => onQuickReply?.(option)}
                >
                  {option.label}
                </button>
              ))}
            </div>
          ) : null}
          {questionHint ? <div className="mt-2 text-xs text-sky-200/80">{questionHint}</div> : null}
        </div>
      ) : null}

      <div
        ref={messagesRef}
        className="chat-messages mb-2 min-h-0 min-w-0 flex-1 overflow-auto p-2 space-y-2"
        onScroll={() => {
          const host = messagesRef.current;
          if (!host) return;
          stickToBottomRef.current = isNearBottom(host);
        }}
      >
        {messages.length ? (
          <AnimatePresence initial={false}>
            {messages.map((message) => {
              const assistantLike = message.role === "assistant" || message.role === "system";
              return (
                <motion.article
                  key={message.id}
                  className={`chat-msg ${assistantLike ? "assistant" : "user"}`}
                  initial={reducedMotion ? false : { opacity: 0, y: 4 }}
                  animate={reducedMotion ? undefined : { opacity: 1, y: 0 }}
                  exit={reducedMotion ? undefined : { opacity: 0, y: -4 }}
                >
                  <div className="chat-msg-role">
                    {assistantLike ? (
                      <span className="inline-flex items-center gap-2">
                        {botIdentity?.botAvatarUrl ? (
                          <img
                            src={botIdentity.botAvatarUrl}
                            alt={assistantLabel}
                            className="chat-avatar-ring h-5 w-5 object-cover"
                          />
                        ) : null}
                        <span>{message.displayRole || assistantLabel}</span>
                      </span>
                    ) : (
                      message.displayRole || message.role
                    )}
                  </div>
                  {message.markdown ? (
                    <div
                      className="tcp-markdown tcp-markdown-ai"
                      dangerouslySetInnerHTML={{ __html: renderMarkdownSafe(message.text || "") }}
                    />
                  ) : (
                    <pre className="chat-msg-pre">{message.text || " "}</pre>
                  )}
                </motion.article>
              );
            })}
          </AnimatePresence>
        ) : (
          <div className="chat-empty-state">
            <p className="chat-rail-empty">{emptyText}</p>
          </div>
        )}

        {showThinking || streamingText ? (
          <article className="chat-msg assistant">
            <div className="chat-msg-role">
              <span className="inline-flex items-center gap-2">
                {botIdentity?.botAvatarUrl ? (
                  <img
                    src={botIdentity.botAvatarUrl}
                    alt={assistantLabel}
                    className="chat-avatar-ring h-5 w-5 object-cover"
                  />
                ) : null}
                <span>{assistantLabel}</span>
              </span>
            </div>
            {showThinking && !streamingText ? (
              <div className="tcp-thinking" aria-live="polite">
                <span>{thinkingText}</span>
                <i></i>
                <i></i>
                <i></i>
              </div>
            ) : null}
            {streamingText ? <pre className="chat-msg-pre">{streamingText}</pre> : null}
          </article>
        ) : null}
      </div>

      {statusTitle ? (
        <div className="mb-2 rounded-xl border border-sky-500/30 bg-sky-950/20 p-3 text-sm text-sky-100">
          <div className="flex items-center gap-2 font-medium">
            <Spinner className="text-sky-200" label={statusTitle} />
            {statusTitle}
          </div>
          {statusDetail ? <div className="mt-1 text-xs text-sky-200/80">{statusDetail}</div> : null}
        </div>
      ) : null}

      <div className="chat-composer shrink-0">
        {composerAccessory ? <div className="mb-2">{composerAccessory}</div> : null}

        {attachments.length ? (
          <div className="chat-attach-row mb-2 flex flex-wrap items-center gap-2">
            <span className="tcp-subtle text-xs">{attachments.length} attached</span>
            <div className="flex flex-wrap gap-1">
              {attachments.map((file, index) => (
                <span key={`${file.path}-${index}`} className="chat-file-pill min-w-0">
                  <span className="chat-file-pill-name" title={file.path}>
                    {file.name || file.path}
                  </span>
                  {file.size != null ? (
                    <span className="chat-file-pill-size">
                      {file.size < 1024
                        ? `${file.size}B`
                        : file.size < 1024 * 1024
                          ? `${(file.size / 1024).toFixed(1)}KB`
                          : `${(file.size / 1024 / 1024).toFixed(1)}MB`}
                    </span>
                  ) : null}
                  {onOpenAttachment ? (
                    <button
                      type="button"
                      className="chat-file-pill-btn"
                      title="Open in Files"
                      aria-label="Open attachment in Files"
                      onClick={() => onOpenAttachment(index)}
                    >
                      <Icon name="folder-open" />
                    </button>
                  ) : null}
                  {onRemoveAttachment ? (
                    <button
                      type="button"
                      className="chat-file-pill-btn chat-file-pill-btn-danger"
                      title="Remove from list"
                      aria-label="Remove attachment from list"
                      onClick={() => onRemoveAttachment(index)}
                    >
                      <Icon name="x" />
                    </button>
                  ) : null}
                </span>
              ))}
            </div>
          </div>
        ) : null}

        <div className="chat-input-wrap">
          {onAttach ? (
            <button
              type="button"
              className="chat-icon-btn chat-icon-btn-inner"
              title="Attach files"
              aria-label="Attach files"
              disabled={attachDisabled}
              onClick={onAttach}
            >
              <Icon name="paperclip" />
            </button>
          ) : null}
          <textarea
            ref={inputRef}
            rows={1}
            value={inputValue}
            className="tcp-input chat-input-with-clip chat-input-modern resize-none"
            placeholder={inputPlaceholder}
            disabled={inputDisabled}
            onInput={(event) => onInputChange((event.target as HTMLTextAreaElement).value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                if (!sendDisabled) onSend();
              }
            }}
          />
          <button
            type="button"
            className="chat-send-btn"
            title={sendLabel}
            aria-label={sendLabel}
            disabled={sendDisabled}
            onClick={onSend}
          >
            <Icon name="send" />
          </button>
        </div>
      </div>
    </div>
  );
}
