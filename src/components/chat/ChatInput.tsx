import { useState, useRef, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Send, Paperclip, Mic, StopCircle, X, FileText } from "lucide-react";
import { Button } from "@/components/ui";
import { cn } from "@/lib/utils";

export interface FileAttachment {
  id: string;
  type: "image" | "file";
  name: string;
  mime: string;
  url: string; // data URL for images, or file path
  size: number;
  preview?: string; // thumbnail for images
}

interface ChatInputProps {
  onSend: (message: string, attachments?: FileAttachment[]) => void;
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
  const [attachments, setAttachments] = useState<FileAttachment[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Auto-resize textarea
  useEffect(() => {
    const textarea = textareaRef.current;
    if (textarea) {
      textarea.style.height = "auto";
      textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`;
    }
  }, [message]);

  const generateId = () => `attach_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`;

  const fileToDataUrl = (file: File): Promise<string> => {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(reader.result as string);
      reader.onerror = reject;
      reader.readAsDataURL(file);
    });
  };

  const addFile = useCallback(async (file: File) => {
    const isImage = file.type.startsWith("image/");
    const dataUrl = await fileToDataUrl(file);

    const attachment: FileAttachment = {
      id: generateId(),
      type: isImage ? "image" : "file",
      name: file.name,
      mime: file.type || "application/octet-stream",
      url: dataUrl,
      size: file.size,
      preview: isImage ? dataUrl : undefined,
    };

    setAttachments((prev) => [...prev, attachment]);
  }, []);

  const handlePaste = useCallback(
    async (e: React.ClipboardEvent) => {
      const items = e.clipboardData?.items;
      if (!items) return;

      for (const item of items) {
        if (item.type.startsWith("image/")) {
          e.preventDefault();
          const file = item.getAsFile();
          if (file) {
            await addFile(file);
          }
        }
      }
    },
    [addFile]
  );

  const handleDrop = useCallback(
    async (e: React.DragEvent) => {
      e.preventDefault();
      setIsDragging(false);

      const files = e.dataTransfer?.files;
      if (!files) return;

      for (const file of files) {
        await addFile(file);
      }
    },
    [addFile]
  );

  const handleDragOver = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(true);
  }, []);

  const handleDragLeave = useCallback((e: React.DragEvent) => {
    e.preventDefault();
    setIsDragging(false);
  }, []);

  const handleFileSelect = useCallback(
    async (e: React.ChangeEvent<HTMLInputElement>) => {
      const files = e.target.files;
      if (!files) return;

      for (const file of files) {
        await addFile(file);
      }

      // Reset input
      if (fileInputRef.current) {
        fileInputRef.current.value = "";
      }
    },
    [addFile]
  );

  const removeAttachment = (id: string) => {
    setAttachments((prev) => prev.filter((a) => a.id !== id));
  };

  const handleSubmit = () => {
    if ((!message.trim() && attachments.length === 0) || disabled) return;
    onSend(message.trim(), attachments.length > 0 ? attachments : undefined);
    setMessage("");
    setAttachments([]);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      handleSubmit();
    }
  };

  const formatFileSize = (bytes: number) => {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  };

  return (
    <motion.div
      className="border-t border-border bg-surface/50 p-4"
      initial={{ y: 20, opacity: 0 }}
      animate={{ y: 0, opacity: 1 }}
      onDrop={handleDrop}
      onDragOver={handleDragOver}
      onDragLeave={handleDragLeave}
    >
      <div className="mx-auto max-w-3xl">
        {/* Drag overlay */}
        <AnimatePresence>
          {isDragging && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              className="absolute inset-0 z-50 flex items-center justify-center bg-primary/10 border-2 border-dashed border-primary rounded-xl"
            >
              <div className="text-center">
                <Paperclip className="h-8 w-8 text-primary mx-auto mb-2" />
                <p className="text-sm font-medium text-primary">Drop files here</p>
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        {/* Attachment previews */}
        <AnimatePresence>
          {attachments.length > 0 && (
            <motion.div
              initial={{ height: 0, opacity: 0 }}
              animate={{ height: "auto", opacity: 1 }}
              exit={{ height: 0, opacity: 0 }}
              className="mb-3 overflow-hidden"
            >
              <div className="flex flex-wrap gap-2">
                {attachments.map((attachment) => (
                  <motion.div
                    key={attachment.id}
                    initial={{ scale: 0.8, opacity: 0 }}
                    animate={{ scale: 1, opacity: 1 }}
                    exit={{ scale: 0.8, opacity: 0 }}
                    className="relative group"
                  >
                    {attachment.type === "image" && attachment.preview ? (
                      <div className="relative w-16 h-16 rounded-lg overflow-hidden border border-border bg-surface">
                        <img
                          src={attachment.preview}
                          alt={attachment.name}
                          className="w-full h-full object-cover"
                        />
                        <button
                          onClick={() => removeAttachment(attachment.id)}
                          className="absolute -top-1 -right-1 p-0.5 bg-error text-white rounded-full opacity-0 group-hover:opacity-100 transition-opacity"
                        >
                          <X className="h-3 w-3" />
                        </button>
                      </div>
                    ) : (
                      <div className="relative flex items-center gap-2 px-3 py-2 rounded-lg border border-border bg-surface">
                        <FileText className="h-4 w-4 text-text-muted" />
                        <div className="max-w-[120px]">
                          <p className="text-xs font-medium text-text truncate">
                            {attachment.name}
                          </p>
                          <p className="text-xs text-text-muted">
                            {formatFileSize(attachment.size)}
                          </p>
                        </div>
                        <button
                          onClick={() => removeAttachment(attachment.id)}
                          className="p-0.5 text-text-muted hover:text-error transition-colors"
                        >
                          <X className="h-3 w-3" />
                        </button>
                      </div>
                    )}
                  </motion.div>
                ))}
              </div>
            </motion.div>
          )}
        </AnimatePresence>

        <div
          className={cn(
            "flex items-end gap-3 rounded-xl border bg-surface-elevated p-3 transition-colors",
            disabled
              ? "border-border opacity-50"
              : isDragging
                ? "border-primary bg-primary/5"
                : "border-border hover:border-border-subtle focus-within:border-primary"
          )}
        >
          {/* Attachment button */}
          <button
            type="button"
            onClick={() => fileInputRef.current?.click()}
            className="flex h-9 w-9 items-center justify-center rounded-lg text-text-subtle transition-colors hover:bg-surface hover:text-text"
            disabled={disabled}
            title="Attach file (or paste image)"
          >
            <Paperclip className="h-5 w-5" />
          </button>

          {/* Hidden file input */}
          <input
            ref={fileInputRef}
            type="file"
            multiple
            accept="image/*,.pdf,.txt,.md,.json,.js,.ts,.jsx,.tsx,.py,.rs,.go,.java,.c,.cpp,.h,.hpp,.css,.html,.xml,.yaml,.yml,.toml"
            onChange={handleFileSelect}
            className="hidden"
          />

          {/* Input area */}
          <div className="flex-1">
            <textarea
              ref={textareaRef}
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              onKeyDown={handleKeyDown}
              onPaste={handlePaste}
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
              disabled={(!message.trim() && attachments.length === 0) || disabled}
              className="h-9 w-9 p-0"
              title="Send message"
            >
              <Send className="h-4 w-4" />
            </Button>
          )}
        </div>

        {/* Hints */}
        <div className="mt-2 flex items-center justify-between text-xs text-text-subtle">
          <span>Press Enter to send, Shift+Enter for new line • Paste images from clipboard</span>
          <span>Tandem can read and write files in your workspace</span>
        </div>
      </div>
    </motion.div>
  );
}
