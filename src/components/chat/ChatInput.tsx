import { useState, useRef, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { Send, Paperclip, StopCircle, X, FileText } from "lucide-react";
import { Button } from "@/components/ui";
import { cn } from "@/lib/utils";
import { ContextToolbar } from "./ContextToolbar";

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
  selectedAgent?: string;
  onAgentChange?: (agent: string | undefined) => void;
  externalAttachment?: FileAttachment | null;
  onExternalAttachmentProcessed?: () => void;
  // New props for Context Toolbar
  enabledToolCategories?: Set<string>;
  onToolCategoriesChange?: (categories: Set<string>) => void;
  activeProviderLabel?: string;
  activeModelLabel?: string;
  allowAllTools?: boolean;
  onAllowAllToolsChange?: (allow: boolean) => void;
  allowAllToolsLocked?: boolean;
  onModelSelect?: (modelId: string, providerId: string) => void;
  // Ralph Loop
  loopEnabled?: boolean;
  onLoopToggle?: (enabled: boolean) => void;
  loopStatus?: any; // Imported type would be better but keeping it simple for now to correspond with ContextToolbar
  onLoopPanelOpen?: () => void;
}

export function ChatInput({
  onSend,
  onStop,
  disabled,
  isGenerating,
  placeholder = "Ask Tandem anything...",
  selectedAgent,
  onAgentChange,
  externalAttachment,
  onExternalAttachmentProcessed,
  enabledToolCategories,
  onToolCategoriesChange,
  activeProviderLabel,
  activeModelLabel,
  allowAllTools,
  onAllowAllToolsChange,
  allowAllToolsLocked,
  onModelSelect,
  loopEnabled,
  onLoopToggle,
  loopStatus,
  onLoopPanelOpen,
}: ChatInputProps) {
  const [message, setMessage] = useState("");
  const [attachments, setAttachments] = useState<FileAttachment[]>([]);
  const [isDragging, setIsDragging] = useState(false);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Auto-focus on mount
  useEffect(() => {
    // Small delay to ensure the component is fully mounted
    const timer = setTimeout(() => {
      textareaRef.current?.focus();
    }, 100);
    return () => clearTimeout(timer);
  }, []);

  // Let parent clear external attachment after we've saved it to local state
  useEffect(() => {
    if (externalAttachment) {
      setAttachments((prev) => {
        const exists = prev.some((a) => a.id === externalAttachment.id);
        if (exists) return prev;
        return [...prev, externalAttachment];
      });
      onExternalAttachmentProcessed?.();
    }
  }, [externalAttachment, onExternalAttachmentProcessed]);

  // Auto-resize textarea
  useEffect(() => {
    const textarea = textareaRef.current;
    if (textarea) {
      textarea.style.height = "auto";
      textarea.style.height = `${Math.min(textarea.scrollHeight, 200)}px`;
    }
  }, [message]);

  const generateId = () => `attach_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`;

  const fileToDataUrl = (file: globalThis.File): Promise<string> => {
    return new Promise((resolve, reject) => {
      const reader = new globalThis.FileReader();
      reader.onload = () => resolve(reader.result as string);
      reader.onerror = reject;
      reader.readAsDataURL(file);
    });
  };

  const processImage = async (file: File | Blob): Promise<{ dataUrl: string; size: number }> => {
    return new Promise((resolve, reject) => {
      const img = new Image();
      const url = URL.createObjectURL(file);

      img.onload = () => {
        URL.revokeObjectURL(url);
        const MAX_WIDTH = 1024;
        const MAX_HEIGHT = 1024;
        let width = img.width;
        let height = img.height;

        if (width > MAX_WIDTH || height > MAX_HEIGHT) {
          const ratio = Math.min(MAX_WIDTH / width, MAX_HEIGHT / height);
          width = Math.round(width * ratio);
          height = Math.round(height * ratio);
        }

        const canvas = document.createElement("canvas");
        canvas.width = width;
        canvas.height = height;
        const ctx = canvas.getContext("2d");
        if (!ctx) {
          reject(new Error("Failed to get canvas context"));
          return;
        }

        ctx.drawImage(img, 0, 0, width, height);

        // JPEG format with 0.8 quality is much smaller than PNG for base64 inlining
        const dataUrl = canvas.toDataURL("image/jpeg", 0.8);

        // Approximate size of base64 content
        const size = Math.round(((dataUrl.length - "data:image/jpeg;base64,".length) * 3) / 4);

        resolve({ dataUrl, size });
      };

      img.onerror = (err) => {
        URL.revokeObjectURL(url);
        reject(err);
      };

      img.src = url;
    });
  };

  const addFile = useCallback(async (file: globalThis.File) => {
    const isImage = file.type.startsWith("image/");
    let dataUrl = "";
    let size = file.size;

    if (isImage) {
      try {
        const processed = await processImage(file);
        dataUrl = processed.dataUrl;
        size = processed.size;
        console.log(`[Image] Original size: ${file.size}, Compressed size: ${size}`);
      } catch (e) {
        console.error("Failed to process image, falling back to raw:", e);
        dataUrl = await fileToDataUrl(file);
      }
    } else {
      dataUrl = await fileToDataUrl(file);
    }

    const attachment: FileAttachment = {
      id: generateId(),
      type: isImage ? "image" : "file",
      name: file.name,
      mime: isImage ? "image/jpeg" : file.type || "application/octet-stream",
      url: dataUrl,
      size: size,
      preview: isImage ? dataUrl : undefined,
    };

    setAttachments((prev) => [...prev, attachment]);
  }, []);

  // Linux clipboard fallback: Use Tauri native clipboard when paste event doesn't work
  useEffect(() => {
    const handleKeyDown = async (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key === "v") {
        console.log("[Clipboard] Ctrl+V pressed, trying Tauri native clipboard...");
        try {
          // Dynamic import to avoid errors if plugin not available
          const clipboard = await import("@tauri-apps/plugin-clipboard-manager");
          console.log("[Clipboard] Plugin loaded, reading image...");

          const imageBytes = await clipboard.readImage();
          console.log("[Clipboard] Read result:", imageBytes ? "Got image data" : "No image data");

          if (imageBytes) {
            e.preventDefault();

            // Get RGBA bytes and dimensions from the image
            const rgbaBytes = await imageBytes.rgba();
            const originalSize = await imageBytes.size();
            console.log(
              "[Clipboard] Image dimensions:",
              originalSize.width,
              "x",
              originalSize.height,
              ", RGBA bytes:",
              rgbaBytes.length
            );

            // Convert RGBA bytes to PNG using Canvas
            const canvas = document.createElement("canvas");
            canvas.width = originalSize.width;
            canvas.height = originalSize.height;
            const ctx = canvas.getContext("2d");

            if (ctx) {
              // Create ImageData from RGBA bytes
              const imageData = new ImageData(
                new Uint8ClampedArray(rgbaBytes),
                originalSize.width,
                originalSize.height
              );
              ctx.putImageData(imageData, 0, 0);

              // Convert canvas to blob then compress it
              canvas.toBlob(async (blob) => {
                if (blob) {
                  const processed = await processImage(blob);

                  const attachment: FileAttachment = {
                    id: generateId(),
                    type: "image",
                    name: `clipboard-${Date.now()}.jpg`,
                    mime: "image/jpeg",
                    url: processed.dataUrl,
                    size: processed.size,
                    preview: processed.dataUrl,
                  };

                  console.log(`[Clipboard] Compressed size: ${processed.size} bytes`);
                  setAttachments((prev) => [...prev, attachment]);
                }
              }, "image/png");
            }
          } else {
            console.log("[Clipboard] No image in clipboard, falling through to web API");
          }
        } catch (err) {
          console.error("[Clipboard] Error reading from Tauri clipboard:", err);
          // Fall through to web clipboard handler
        }
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [addFile]);

  // File browser state
  const handlePaste = useCallback(
    async (e: React.ClipboardEvent) => {
      const clipboardData = e.clipboardData;
      if (!clipboardData) return;

      // Try clipboardData.items (standard way, works on Windows)
      const items = clipboardData.items;
      if (items && items.length > 0) {
        for (const item of items) {
          if (item.type.startsWith("image/")) {
            e.preventDefault();
            const file = item.getAsFile();
            if (file) {
              console.log("[Paste] Image from clipboardData.items:", file.type, file.size);
              await addFile(file);
              return; // Successfully processed
            }
          }
        }
      }

      // Fallback: Try clipboardData.files (Linux WebKit sometimes puts images here)
      const files = clipboardData.files;
      if (files && files.length > 0) {
        for (const file of files) {
          if (file.type.startsWith("image/")) {
            e.preventDefault();
            console.log("[Paste] Image from clipboardData.files:", file.type, file.size);
            await addFile(file);
            return; // Successfully processed
          }
        }
      }

      // Also try clipboard.types for debugging
      if (clipboardData.types && clipboardData.types.length > 0) {
        console.log("[Paste] Available clipboard types:", clipboardData.types);
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
    if (externalAttachment?.id === id) {
      onExternalAttachmentProcessed?.();
    }
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
      <div className="mx-auto w-full max-w-5xl">
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
              <div className="flex flex-wrap gap-2 p-1">
                {attachments.map((attachment) => (
                  <motion.div
                    key={attachment.id}
                    initial={{ scale: 0.8, opacity: 0 }}
                    animate={{ scale: 1, opacity: 1 }}
                    exit={{ scale: 0.8, opacity: 0 }}
                    className="relative group"
                  >
                    {attachment.type === "image" &&
                    attachment.preview &&
                    attachment.preview !== "" ? (
                      <>
                        <div className="w-16 h-16 rounded-lg overflow-hidden border border-border bg-surface">
                          <img
                            src={attachment.preview}
                            alt={attachment.name}
                            className="w-full h-full object-cover"
                          />
                        </div>
                        <button
                          onClick={() => removeAttachment(attachment.id)}
                          className="absolute -top-1 -right-1 p-0.5 bg-error text-white rounded-full opacity-0 group-hover:opacity-100 transition-opacity"
                        >
                          <X className="h-3 w-3" />
                        </button>
                      </>
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

          {/* Voice input button - Disabled (not implemented yet) */}
          {/* <button
            type="button"
            className="flex h-9 w-9 items-center justify-center rounded-lg text-text-subtle transition-colors hover:bg-surface hover:text-text"
            disabled={disabled}
            title="Voice input"
          >
            <Mic className="h-5 w-5" />
          </button> */}

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

        {/* Context Toolbar - Agent, Tools, Model selectors */}
        <ContextToolbar
          selectedAgent={selectedAgent}
          onAgentChange={onAgentChange}
          enabledToolCategories={enabledToolCategories || new Set()}
          onToolCategoriesChange={onToolCategoriesChange || (() => {})}
          activeProviderLabel={activeProviderLabel}
          activeModelLabel={activeModelLabel}
          allowAllTools={allowAllTools}
          onAllowAllToolsChange={onAllowAllToolsChange}
          allowAllToolsLocked={allowAllToolsLocked}
          onModelSelect={onModelSelect}
          disabled={disabled}
          loopEnabled={loopEnabled}
          onLoopToggle={onLoopToggle}
          loopStatus={loopStatus}
          onLoopPanelOpen={onLoopPanelOpen}
        />
      </div>
    </motion.div>
  );
}
