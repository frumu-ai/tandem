import { useState, useRef, useEffect } from "react";
import { motion } from "framer-motion";
import { Send, Paperclip, Mic, StopCircle } from "lucide-react";
import { Button } from "@/components/ui";
import { cn } from "@/lib/utils";

interface ChatInputProps {
  onSend: (message: string) => void;
  onStop?: () => void;
  disabled?: boolean;
  isGenerating?: boolean;
  placeholder?: string;
}

export function ChatInput({
  onSend,
  onStop,
  disabled,
  isGenerating,
  placeholder = "Ask Tandem anything...",
}: ChatInputProps) {
  const [message, setMessage] = useState("");
  const textareaRef = useRef<HTMLTextAreaElement>(null);

  // Auto-resize textarea
  useEffect(() => {
    const textarea = textareaRef.current;
    if (textarea) {
      textarea.style.height = "auto";
      textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`;
    }
  }, [message]);

  const handleSubmit = () => {
    if (!message.trim() || disabled) return;
    onSend(message.trim());
    setMessage("");
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  return (
    <motion.div
      className="border-t border-border bg-surface/50 p-4"
      initial={{ y: 20, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
    >
      <div className="mx-auto max-w-3xl">
        <div
          className={cn(
            "flex items-end gap-3 rounded-xl border bg-surface-elevated p-3 transition-colors",
            disabled ? "border-border opacity-50" : "border-border hover:border-border-subtle focus-within:border-primary"
          )}
        >
          {/* Attachment button */}
          <button
            type="button"
            className="flex h-9 w-9 items-center justify-center rounded-lg text-text-subtle transition-colors hover:bg-surface hover:text-text"
            disabled={disabled}
            title="Attach file"
          >
            <Paperclip className="h-5 w-5" />
          </button>

          {/* Input area */}
          <div className="flex-1">
            <textarea
              ref={textareaRef}
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder={placeholder}
              disabled={disabled}
              rows={1}
              className="max-h-[200px] w-full resize-none bg-transparent text-text placeholder:text-text-subtle focus:outline-none disabled:cursor-not-allowed"
            />
          </div>

          {/* Voice input button */}
          <button
            type="button"
            className="flex h-9 w-9 items-center justify-center rounded-lg text-text-subtle transition-colors hover:bg-surface hover:text-text"
            disabled={disabled}
            title="Voice input"
          >
            <Mic className="h-5 w-5" />
          </button>

          {/* Send/Stop button */}
          {isGenerating ? (
            <Button
              variant="danger"
              size="sm"
              onClick={onStop}
              className="h-9 w-9 p-0"
              title="Stop generating"
            >
              <StopCircle className="h-5 w-5" />
            </Button>
          ) : (
            <Button
              size="sm"
              onClick={handleSubmit}
              disabled={!message.trim() || disabled}
              className="h-9 w-9 p-0"
              title="Send message"
            >
              <Send className="h-4 w-4" />
            </Button>
          )}
        </div>

        {/* Hints */}
        <div className="mt-2 flex items-center justify-between text-xs text-text-subtle">
          <span>Press Enter to send, Shift+Enter for new line</span>
          <span>Tandem can read and write files in your workspace</span>
        </div>
      </div>
    </motion.div>
  );
}
