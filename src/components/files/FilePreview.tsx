import { useState, useEffect } from "react";
import { motion } from "framer-motion";
import { convertFileSrc } from "@tauri-apps/api/core";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark } from "react-syntax-highlighter/dist/esm/styles/prism";
import {
  X,
  FileText,
  FileCode,
  Image as ImageIcon,
  File,
  Loader2,
  AlertCircle,
  MessageSquarePlus,
  LayoutTemplate,
  ExternalLink,
} from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  readFileContent,
  readFileText,
  readBinaryFile,
  type FileEntry,
  logFrontendError,
} from "@/lib/tauri";
import { PresentationPreview } from "./PresentationPreview";
import { HtmlPreview } from "./HtmlPreview";
// import { cn } from "@/lib/utils"; // Unused

interface FilePreviewProps {
  file: FileEntry;
  onClose: () => void;
  onAddToChat?: (file: FileEntry) => void;
}

type PreviewType =
  | "code"
  | "markdown"
  | "image"
  | "pdf"
  | "text"
  | "binary"
  | "presentation"
  | "html";

const CODE_EXTENSIONS = new Set([
  "ts",
  "tsx",
  "js",
  "jsx",
  "rs",
  "py",
  "java",
  "c",
  "cpp",
  "h",
  "hpp",
  "go",
  "rb",
  "php",
  "swift",
  "kt",
  "scala",
  "sh",
  "bash",
  "css",
  "scss",
  "xml",
  "sql",
  "r",
]);

const IMAGE_EXTENSIONS = new Set(["png", "jpg", "jpeg", "gif", "svg", "webp", "bmp", "ico"]);

const TEXT_EXTENSIONS = new Set([
  "txt",
  "log",
  "csv",
  "json",
  "yaml",
  "yml",
  "toml",
  "ini",
  "cfg",
  "conf",
]);

const EXTRACTABLE_DOC_EXTENSIONS = new Set([
  "pdf",
  "docx",
  "pptx",
  "xlsx",
  "xls",
  "ods",
  "xlsb",
  "rtf",
]);

function getPreviewType(file: FileEntry): PreviewType {
  // Check for presentation files first (full filename match)
  if (file.name.endsWith(".tandem.ppt.json")) return "presentation";

  const ext = file.extension?.toLowerCase();
  if (!ext) return "binary";

  if (ext === "html" || ext === "htm") return "html";
  if (ext === "md") return "markdown";
  if (ext === "pdf") return "pdf";
  if (IMAGE_EXTENSIONS.has(ext)) return "image";
  if (CODE_EXTENSIONS.has(ext)) return "code";
  if (TEXT_EXTENSIONS.has(ext)) return "text";
  if (EXTRACTABLE_DOC_EXTENSIONS.has(ext)) return "text";
  return "binary";
}

function getLanguageFromExtension(ext: string | undefined): string {
  if (!ext) return "text";
  const map: Record<string, string> = {
    ts: "typescript",
    tsx: "tsx",
    js: "javascript",
    jsx: "jsx",
    rs: "rust",
    py: "python",
    cpp: "cpp",
    c: "c",
    h: "c",
    hpp: "cpp",
    java: "java",
    go: "go",
    rb: "ruby",
    php: "php",
    swift: "swift",
    kt: "kotlin",
    scala: "scala",
    sh: "bash",
    bash: "bash",
    css: "css",
    scss: "scss",
    html: "html",
    xml: "xml",
    sql: "sql",
    json: "json",
    yaml: "yaml",
    yml: "yaml",
    toml: "toml",
  };
  return map[ext.toLowerCase()] || ext.toLowerCase();
}

export function FilePreview({ file, onClose, onAddToChat }: FilePreviewProps) {
  // Special case: route .tandem.ppt.json files to PresentationPreview
  const previewType = getPreviewType(file);
  const [content, setContent] = useState<string>("");
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [imageDataUrl, setImageDataUrl] = useState<string | null>(null);
  const [pdfDataUrl, setPdfDataUrl] = useState<string | null>(null);

  // Helper to get MIME type from extension
  const getMimeType = (ext: string): string => {
    const mimeMap: Record<string, string> = {
      png: "image/png",
      jpg: "image/jpeg",
      jpeg: "image/jpeg",
      gif: "image/gif",
      svg: "image/svg+xml",
      webp: "image/webp",
      bmp: "image/bmp",
      ico: "image/x-icon",
    };
    return mimeMap[ext.toLowerCase()] || "image/png";
  };

  // Load images as base64 data URLs
  useEffect(() => {
    if (previewType === "image") {
      const loadImage = async () => {
        try {
          setIsLoading(true);
          setError(null);
          const base64 = await readBinaryFile(file.path);
          const ext = file.extension?.toLowerCase() || "png";
          const mimeType = getMimeType(ext);
          setImageDataUrl(`data:${mimeType};base64,${base64}`);
        } catch (err) {
          console.error("Failed to load image:", err);
          logFrontendError(`Failed to load image: ${file.name}`, `Error: ${err}`);
          setError(`Failed to load image: ${file.name}`);
        } finally {
          setIsLoading(false);
        }
      };
      loadImage();
      return;
    }
  }, [file.path, previewType, file.name, file.extension]);

  // Load PDFs as base64 data URLs
  useEffect(() => {
    if (previewType === "pdf") {
      const loadPdf = async () => {
        try {
          setIsLoading(true);
          setError(null);
          const base64 = await readBinaryFile(file.path);
          setPdfDataUrl(`data:application/pdf;base64,${base64}`);
        } catch (err) {
          console.error("Failed to load PDF:", err);
          logFrontendError(`Failed to load PDF: ${file.name}`, `Error: ${err}`);
          setError(`Failed to load PDF: ${file.name}`);
        } finally {
          setIsLoading(false);
        }
      };
      loadPdf();
      return;
    }
  }, [file.path, previewType, file.name]);

  useEffect(() => {
    if (previewType === "binary") {
      setIsLoading(false);
      return;
    }

    if (previewType === "image" || previewType === "pdf") {
      // Images and PDFs are handled by separate useEffects above
      return;
    }

    const loadContent = async () => {
      try {
        setIsLoading(true);
        setError(null);
        const ext = file.extension?.toLowerCase();
        const fileContent =
          ext && EXTRACTABLE_DOC_EXTENSIONS.has(ext)
            ? await readFileText(file.path, 25 * 1024 * 1024, 200_000)
            : await readFileContent(file.path);
        setContent(fileContent);
      } catch (err) {
        console.error("Failed to load file content:", err);
        setError(err instanceof Error ? err.message : "Failed to load file");
      } finally {
        setIsLoading(false);
      }
    };

    loadContent();
  }, [file.path, previewType]);

  if (previewType === "presentation") {
    return <PresentationPreview file={file} onClose={onClose} />;
  }

  const renderPreview = () => {
    if (isLoading) {
      return (
        <div className="flex h-full items-center justify-center">
          <Loader2 className="h-8 w-8 animate-spin text-primary" />
        </div>
      );
    }

    if (error) {
      return (
        <div className="flex h-full items-center justify-center p-8">
          <div className="text-center">
            <AlertCircle className="mx-auto h-12 w-12 text-red-400" />
            <p className="mt-4 text-sm text-red-400">{error}</p>
          </div>
        </div>
      );
    }

    switch (previewType) {
      case "html":
        // eslint-disable-next-line no-case-declarations
        const parentDir = file.path.substring(0, file.path.lastIndexOf(file.name));
        // eslint-disable-next-line no-case-declarations
        const baseHref = convertFileSrc(parentDir);
        return <HtmlPreview content={content} baseHref={baseHref} />;

      case "image":
        return (
          <div className="flex h-full items-center justify-center p-4 bg-surface">
            {imageDataUrl ? (
              <img
                src={imageDataUrl}
                alt={file.name}
                className="max-h-full max-w-full object-contain rounded"
              />
            ) : null}
          </div>
        );

      case "pdf":
        return (
          <div className="h-full w-full bg-surface">
            {pdfDataUrl ? (
              <embed src={pdfDataUrl} type="application/pdf" className="h-full w-full" />
            ) : null}
          </div>
        );

      case "markdown":
        return (
          <div className="h-full overflow-y-auto p-6">
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
                            <span className="ml-2 text-[0.65rem] uppercase tracking-widest text-text-subtle">
                              {match[1]}
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
          </div>
        );

      case "code":
        return (
          <div className="h-full overflow-y-auto">
            <SyntaxHighlighter
              language={getLanguageFromExtension(file.extension)}
              style={oneDark}
              showLineNumbers
              customStyle={{
                margin: 0,
                padding: "1.5rem",
                background: "transparent",
                fontSize: "0.875rem",
              }}
            >
              {content}
            </SyntaxHighlighter>
          </div>
        );

      case "text":
        return (
          <div className="h-full overflow-y-auto p-6">
            <pre className="text-sm text-text whitespace-pre-wrap font-mono">{content}</pre>
          </div>
        );

      case "binary":
        return (
          <div className="flex h-full items-center justify-center p-8">
            <div className="text-center">
              <File className="mx-auto h-12 w-12 text-text-muted opacity-50" />
              <p className="mt-4 text-sm text-text-muted">Binary file</p>
              <p className="mt-2 text-xs text-text-muted">
                {file.size !== undefined ? formatFileSize(file.size) : "Unknown size"}
              </p>
            </div>
          </div>
        );

      default:
        return null;
    }
  };

  const getIcon = () => {
    switch (previewType) {
      case "html":
        return LayoutTemplate;
      case "image":
        return ImageIcon;
      case "pdf":
        return FileText;
      case "code":
        return FileCode;
      default:
        return FileText;
    }
  };

  const Icon = getIcon();

  return (
    <motion.div
      initial={{ opacity: 0, y: 20 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: 20 }}
      transition={{ duration: 0.2 }}
      className="flex h-full flex-col border-l border-border bg-background"
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-border bg-surface px-4 py-3">
        <div className="flex items-center gap-3 min-w-0 flex-1">
          <Icon className="h-5 w-5 flex-shrink-0 text-primary" />
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium text-text truncate">{file.name}</p>
            <p className="text-xs text-text-muted truncate">{file.path}</p>
          </div>
        </div>
        <div className="flex items-center gap-2 flex-shrink-0">
          {previewType === "html" && (
            <button
              onClick={() => openPath(file.path)}
              className="flex items-center gap-2 rounded-lg bg-surface-elevated px-3 py-1.5 text-sm font-medium text-text transition-colors hover:bg-surface-elevated/80 border border-border"
              title="Open in default browser"
            >
              <ExternalLink className="h-4 w-4" />
              <span>Open in Browser</span>
            </button>
          )}
          {onAddToChat && (
            <button
              onClick={() => onAddToChat(file)}
              className="flex items-center gap-2 rounded-lg bg-primary px-3 py-1.5 text-sm font-medium text-white transition-colors hover:bg-primary/90"
              title="Add to chat context"
            >
              <MessageSquarePlus className="h-4 w-4" />
              <span>Add to Chat</span>
            </button>
          )}
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
            title="Close preview"
          >
            <X className="h-5 w-5" />
          </button>
        </div>
      </div>

      {/* Content */}
      <div className="flex-1 overflow-hidden">{renderPreview()}</div>
    </motion.div>
  );
}

function formatFileSize(bytes: number): string {
  if (bytes === 0) return "0 B";
  const k = 1024;
  const sizes = ["B", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return `${Math.round((bytes / Math.pow(k, i)) * 10) / 10} ${sizes[i]}`;
}
