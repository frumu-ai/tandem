import { useState, useEffect, useCallback } from "react";
import { motion, AnimatePresence } from "framer-motion";
import {
  X,
  Maximize2,
  Minimize2,
  ChevronLeft,
  ChevronRight,
  Download,
  Loader2,
  AlertCircle,
  FileText,
  Presentation as PresentationIcon,
} from "lucide-react";
import { Button } from "@/components/ui";
import { readFileContent, exportPresentation, logFrontendError, type FileEntry } from "@/lib/tauri";
import type { Presentation, Slide, PresentationTheme } from "@/lib/presentation";
import { save } from "@tauri-apps/plugin-dialog";

interface PresentationPreviewProps {
  file: FileEntry;
  onClose: () => void;
  expanded?: boolean;
  onToggleExpand?: () => void;
}

export function PresentationPreview({
  file,
  onClose,
  expanded,
  onToggleExpand,
}: PresentationPreviewProps) {
  const [presentation, setPresentation] = useState<Presentation | null>(null);
  const [currentSlideIndex, setCurrentSlideIndex] = useState(0);
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [isExporting, setIsExporting] = useState(false);
  const [showNotes, setShowNotes] = useState(false);
  const [autoExportComplete, setAutoExportComplete] = useState(false);

  useEffect(() => {
    const loadPresentation = async () => {
      try {
        setIsLoading(true);
        setError(null);
        const content = await readFileContent(file.path);
        const parsed = JSON.parse(content) as Presentation;
        setPresentation(parsed);
      } catch (err) {
        console.error("Failed to load presentation:", err);
        logFrontendError(`Failed to load presentation: ${file.name}`, `Error: ${err}`);
        setError(err instanceof Error ? err.message : "Failed to load presentation");
      } finally {
        setIsLoading(false);
      }
    };

    loadPresentation();
  }, [file.path, file.name]);

  // Auto-export to .pptx on first load (only once)
  useEffect(() => {
    const autoExport = async () => {
      if (!presentation || autoExportComplete) return;

      try {
        console.log("[PresentationPreview] Auto-exporting to .pptx");
        const outputPath = file.path.replace(".tandem.ppt.json", ".pptx");
        await exportPresentation(file.path, outputPath);
        console.log("[PresentationPreview] Auto-exported to:", outputPath);
        setAutoExportComplete(true);
      } catch (err) {
        console.error("[PresentationPreview] Auto-export failed:", err);
        // Don't show error to user - manual export still available
        logFrontendError("Auto-export failed (non-critical)", `${err}`);
      }
    };

    autoExport();
  }, [presentation, file.path, autoExportComplete]);

  const handleExport = async () => {
    if (!presentation) return;

    try {
      setIsExporting(true);
      setError(null);

      // Prompt user for save location
      const outputPath = await save({
        defaultPath: `${file.name.replace(".tandem.ppt.json", "")}.pptx`,
        filters: [
          {
            name: "PowerPoint Presentation",
            extensions: ["pptx"],
          },
        ],
      });

      if (!outputPath) {
        setIsExporting(false);
        return; // User cancelled
      }

      // Export via Rust backend
      const result = await exportPresentation(file.path, outputPath);
      console.log("Export result:", result);

      // Success notification could go here
      window.alert(`Successfully exported to ${outputPath}`);
    } catch (err) {
      console.error("Failed to export presentation:", err);
      setError(err instanceof Error ? err.message : "Failed to export presentation");
    } finally {
      setIsExporting(false);
    }
  };

  const handlePrevSlide = useCallback(() => {
    setCurrentSlideIndex((prev) => Math.max(0, prev - 1));
  }, []);

  const handleNextSlide = useCallback(() => {
    if (!presentation) return;
    setCurrentSlideIndex((prev) => Math.min(presentation.slides.length - 1, prev + 1));
  }, [presentation]);

  const handleKeyDown = useCallback(
    (e: globalThis.KeyboardEvent) => {
      if (e.key === "ArrowLeft") handlePrevSlide();
      if (e.key === "ArrowRight") handleNextSlide();
    },
    [handlePrevSlide, handleNextSlide]
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  if (isLoading) {
    return (
      <div
        className={`flex h-full items-center justify-center bg-background ${expanded ? "" : "border-l border-border"}`}
      >
        <Loader2 className="h-8 w-8 animate-spin text-primary" />
      </div>
    );
  }

  if (error || !presentation) {
    return (
      <div
        className={`flex h-full flex-col bg-background ${expanded ? "" : "border-l border-border"}`}
      >
        <div className="flex items-center justify-between border-b border-border bg-surface px-4 py-3">
          <div className="flex items-center gap-3">
            <AlertCircle className="h-5 w-5 text-error" />
            <p className="text-sm font-medium text-text">Error Loading Presentation</p>
          </div>
          <div className="flex items-center gap-2">
            <button
              onClick={onToggleExpand}
              className="rounded-lg p-1.5 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
              title={expanded ? "Dock preview" : "Expand preview to full screen"}
              disabled={!onToggleExpand}
            >
              {expanded ? <Minimize2 className="h-5 w-5" /> : <Maximize2 className="h-5 w-5" />}
            </button>
            <button
              onClick={onClose}
              className="rounded-lg p-1.5 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
              title="Close preview"
            >
              <X className="h-5 w-5" />
            </button>
          </div>
        </div>
        <div className="flex h-full items-center justify-center p-8">
          <div className="text-center">
            <AlertCircle className="mx-auto h-12 w-12 text-error" />
            <p className="mt-4 text-sm text-error">{error || "Failed to load presentation"}</p>
          </div>
        </div>
      </div>
    );
  }

  const currentSlide = presentation.slides[currentSlideIndex];
  const theme = presentation.theme || "light";

  return (
    <motion.div
      initial={{ opacity: 0, x: 20 }}
      animate={{ opacity: 1, x: 0 }}
      exit={{ opacity: 0, x: 20 }}
      transition={{ duration: 0.2 }}
      className={`flex h-full flex-col bg-background ${expanded ? "" : "border-l border-border"}`}
    >
      {/* Header */}
      <div className="flex items-center justify-between border-b border-border bg-surface px-4 py-3">
        <div className="flex items-center gap-3 min-w-0 flex-1">
          <PresentationIcon className="h-5 w-5 flex-shrink-0 text-primary" />
          <div className="min-w-0 flex-1">
            <p className="text-sm font-medium text-text truncate">{presentation.title}</p>
            <p className="text-xs text-text-muted truncate">
              {presentation.slides.length} slide{presentation.slides.length !== 1 ? "s" : ""}
              {presentation.author && ` • ${presentation.author}`}
            </p>
          </div>
        </div>
        <div className="flex items-center gap-2 flex-shrink-0">
          <button
            onClick={onToggleExpand}
            className="rounded-lg p-1.5 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text disabled:opacity-50"
            title={expanded ? "Dock preview" : "Expand preview to full screen"}
            disabled={!onToggleExpand}
          >
            {expanded ? <Minimize2 className="h-5 w-5" /> : <Maximize2 className="h-5 w-5" />}
          </button>
          <Button
            onClick={handleExport}
            disabled={isExporting}
            size="sm"
            className="flex items-center gap-2"
          >
            {isExporting ? (
              <>
                <Loader2 className="h-4 w-4 animate-spin" />
                <span>Exporting...</span>
              </>
            ) : (
              <>
                <Download className="h-4 w-4" />
                <span>Export to PPTX</span>
              </>
            )}
          </Button>
          <button
            onClick={onClose}
            className="rounded-lg p-1.5 text-text-muted transition-colors hover:bg-surface-elevated hover:text-text"
            title="Close preview"
          >
            <X className="h-5 w-5" />
          </button>
        </div>
      </div>

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

      {/* Main content */}
      <div className="flex-1 overflow-hidden flex flex-col">
        {/* Slide preview */}
        <div className="flex-1 flex items-center justify-center p-8 bg-surface/30">
          <SlideRenderer slide={currentSlide} theme={theme} />
        </div>

        {/* Navigation */}
        <div className="border-t border-border bg-surface px-4 py-3">
          <div className="flex items-center justify-between">
            <Button
              onClick={handlePrevSlide}
              disabled={currentSlideIndex === 0}
              size="sm"
              variant="secondary"
            >
              <ChevronLeft className="h-4 w-4" />
              <span>Previous</span>
            </Button>

            <div className="text-sm text-text-muted">
              Slide {currentSlideIndex + 1} of {presentation.slides.length}
            </div>

            <Button
              onClick={handleNextSlide}
              disabled={currentSlideIndex === presentation.slides.length - 1}
              size="sm"
              variant="secondary"
            >
              <span>Next</span>
              <ChevronRight className="h-4 w-4" />
            </Button>
          </div>
        </div>

        {/* Speaker notes (if present) */}
        {currentSlide.notes && (
          <div className="border-t border-border bg-surface-elevated">
            <button
              onClick={() => setShowNotes(!showNotes)}
              className="w-full flex items-center justify-between px-4 py-2 text-xs font-medium text-text-muted hover:text-text transition-colors"
            >
              <div className="flex items-center gap-2">
                <FileText className="h-3.5 w-3.5" />
                <span>Speaker Notes</span>
              </div>
              <ChevronRight
                className={`h-3 w-3 transition-transform ${showNotes ? "rotate-90" : ""}`}
              />
            </button>
            <AnimatePresence>
              {showNotes && (
                <motion.div
                  initial={{ height: 0, opacity: 0 }}
                  animate={{ height: "auto", opacity: 1 }}
                  exit={{ height: 0, opacity: 0 }}
                  className="overflow-hidden"
                >
                  <div className="px-4 pb-3 text-xs text-text-muted whitespace-pre-wrap">
                    {currentSlide.notes}
                  </div>
                </motion.div>
              )}
            </AnimatePresence>
          </div>
        )}

        {/* Slide thumbnails */}
        <div className="border-t border-border bg-surface p-3 overflow-x-auto">
          <div className="flex gap-2">
            {presentation.slides.map((slide, index) => (
              <button
                key={slide.id}
                onClick={() => setCurrentSlideIndex(index)}
                className={`flex-shrink-0 w-24 h-16 rounded border-2 transition-all overflow-hidden ${
                  index === currentSlideIndex
                    ? "border-primary ring-2 ring-primary/20"
                    : "border-border hover:border-border-subtle"
                }`}
                title={slide.title || `Slide ${index + 1}`}
              >
                <SlideThumbnail slide={slide} theme={theme} />
              </button>
            ))}
          </div>
        </div>
      </div>
    </motion.div>
  );
}

interface SlideRendererProps {
  slide: Slide;
  theme: PresentationTheme;
}

function SlideRenderer({ slide, theme }: SlideRendererProps) {
  const themeClasses = getThemeClasses(theme);

  return (
    <div
      className={`w-full max-w-4xl aspect-video rounded-lg shadow-2xl overflow-hidden ${themeClasses.background}`}
    >
      <div className="h-full flex flex-col p-12">
        {slide.layout === "title" && (
          <div className="flex-1 flex flex-col items-center justify-center text-center gap-4">
            {slide.title && (
              <h1 className={`text-5xl font-bold ${themeClasses.title}`}>{slide.title}</h1>
            )}
            {slide.subtitle && (
              <p className={`text-2xl ${themeClasses.subtitle}`}>{slide.subtitle}</p>
            )}
          </div>
        )}

        {slide.layout === "section" && (
          <div className="flex-1 flex flex-col items-center justify-center text-center gap-3">
            {slide.title && (
              <h2 className={`text-4xl font-bold ${themeClasses.title}`}>{slide.title}</h2>
            )}
            {slide.subtitle && (
              <p className={`text-xl ${themeClasses.subtitle}`}>{slide.subtitle}</p>
            )}
          </div>
        )}

        {slide.layout === "content" && (
          <>
            {slide.title && (
              <h2 className={`text-3xl font-bold mb-6 ${themeClasses.title}`}>{slide.title}</h2>
            )}
            <div className="flex-1 flex flex-col gap-4">
              {slide.elements.map((element, idx) => (
                <div key={idx}>
                  {element.type === "bullet_list" && Array.isArray(element.content) && (
                    <ul className="space-y-3">
                      {element.content.map((bullet, bulletIdx) => (
                        <li
                          key={bulletIdx}
                          className={`text-xl flex items-start gap-3 ${themeClasses.text}`}
                        >
                          <span
                            className={`mt-1 h-2 w-2 rounded-full flex-shrink-0 ${themeClasses.bullet}`}
                          />
                          <span>{bullet}</span>
                        </li>
                      ))}
                    </ul>
                  )}
                  {element.type === "text" && typeof element.content === "string" && (
                    <p className={`text-lg ${themeClasses.text}`}>{element.content}</p>
                  )}
                </div>
              ))}
            </div>
          </>
        )}

        {slide.layout === "blank" && (
          <div className="flex-1 flex items-center justify-center">
            <p className={`text-text-muted`}>Blank Slide</p>
          </div>
        )}
      </div>
    </div>
  );
}

interface SlideThumbnailProps {
  slide: Slide;
  theme: PresentationTheme;
}

function SlideThumbnail({ slide, theme }: SlideThumbnailProps) {
  const themeClasses = getThemeClasses(theme);

  return (
    <div className={`w-full h-full ${themeClasses.background} p-2 overflow-hidden`}>
      {slide.layout === "title" && (
        <div className="flex flex-col items-center justify-center h-full">
          {slide.title && (
            <div
              className={`text-[0.45rem] font-bold text-center line-clamp-2 ${themeClasses.title}`}
            >
              {slide.title}
            </div>
          )}
        </div>
      )}

      {slide.layout === "section" && (
        <div className="flex flex-col items-center justify-center h-full">
          {slide.title && (
            <div
              className={`text-[0.4rem] font-bold text-center line-clamp-2 ${themeClasses.title}`}
            >
              {slide.title}
            </div>
          )}
        </div>
      )}

      {slide.layout === "content" && (
        <div className="h-full flex flex-col gap-1">
          {slide.title && (
            <div className={`text-[0.4rem] font-bold line-clamp-1 ${themeClasses.title}`}>
              {slide.title}
            </div>
          )}
          {slide.elements.map((element, idx) => (
            <div key={idx} className="text-[0.3rem]">
              {element.type === "bullet_list" && Array.isArray(element.content) && (
                <div className="space-y-0.5">
                  {element.content.slice(0, 3).map((bullet, bulletIdx) => (
                    <div key={bulletIdx} className={`flex gap-1 ${themeClasses.text}`}>
                      <span
                        className={`mt-0.5 h-0.5 w-0.5 rounded-full flex-shrink-0 ${themeClasses.bullet}`}
                      />
                      <span className="line-clamp-1">{bullet}</span>
                    </div>
                  ))}
                </div>
              )}
            </div>
          ))}
        </div>
      )}

      {slide.layout === "blank" && (
        <div className="h-full flex items-center justify-center">
          <div className="text-[0.3rem] text-text-muted">Blank</div>
        </div>
      )}
    </div>
  );
}

function getThemeClasses(theme: PresentationTheme) {
  switch (theme) {
    case "dark":
      return {
        background: "bg-gray-900",
        title: "text-white",
        subtitle: "text-gray-300",
        text: "text-gray-200",
        bullet: "bg-blue-400",
      };
    case "corporate":
      return {
        background: "bg-gradient-to-br from-blue-900 to-blue-800",
        title: "text-white",
        subtitle: "text-blue-100",
        text: "text-blue-50",
        bullet: "bg-blue-300",
      };
    case "minimal":
      return {
        background: "bg-white",
        title: "text-gray-900",
        subtitle: "text-gray-600",
        text: "text-gray-700",
        bullet: "bg-gray-900",
      };
    case "light":
    default:
      return {
        background: "bg-gradient-to-br from-white to-gray-50",
        title: "text-gray-900",
        subtitle: "text-gray-600",
        text: "text-gray-800",
        bullet: "bg-blue-500",
      };
  }
}
