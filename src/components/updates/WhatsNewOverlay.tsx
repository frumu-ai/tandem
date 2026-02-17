import { AnimatePresence, motion } from "framer-motion";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

interface WhatsNewOverlayProps {
  open: boolean;
  version: string;
  markdown: string;
  onClose: () => void;
}

export function WhatsNewOverlay({ open, version, markdown, onClose }: WhatsNewOverlayProps) {
  if (!open) return null;

  return (
    <AnimatePresence>
      <motion.div
        className="fixed inset-0 z-[90] flex items-center justify-center bg-surface/80 backdrop-blur-sm"
        initial={{ opacity: 0 }}
        animate={{ opacity: 1 }}
        exit={{ opacity: 0 }}
      >
        <motion.div
          className="mx-4 max-h-[85vh] w-full max-w-3xl overflow-hidden rounded-2xl border border-border bg-surface shadow-2xl"
          initial={{ y: 12, opacity: 0 }}
          animate={{ y: 0, opacity: 1 }}
          exit={{ y: 8, opacity: 0 }}
        >
          <div className="flex items-center justify-between border-b border-border px-6 py-4">
            <div>
              <h2 className="text-lg font-semibold text-text">Welcome to Tandem {version}</h2>
              <p className="text-sm text-text-muted">What&apos;s new in this release</p>
            </div>
            <button
              type="button"
              onClick={onClose}
              className="rounded-lg border border-border bg-surface-elevated px-3 py-2 text-sm text-text transition-colors hover:bg-surface"
            >
              Continue
            </button>
          </div>

          <div className="max-h-[70vh] overflow-y-auto px-6 py-5">
            <article className="max-w-none text-text">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={{
                  h1: ({ ...props }) => <h1 className="mb-4 text-2xl font-bold" {...props} />,
                  h2: ({ ...props }) => (
                    <h2 className="mb-3 mt-6 text-xl font-semibold" {...props} />
                  ),
                  h3: ({ ...props }) => (
                    <h3 className="mb-2 mt-4 text-lg font-semibold" {...props} />
                  ),
                  p: ({ ...props }) => <p className="mb-3 leading-7 text-text" {...props} />,
                  ul: ({ ...props }) => <ul className="mb-4 list-disc pl-6" {...props} />,
                  ol: ({ ...props }) => <ol className="mb-4 list-decimal pl-6" {...props} />,
                  li: ({ ...props }) => <li className="mb-1 leading-7 text-text" {...props} />,
                  strong: ({ ...props }) => (
                    <strong className="font-semibold text-text" {...props} />
                  ),
                  code: ({ ...props }) => (
                    <code
                      className="rounded bg-surface-elevated px-1.5 py-0.5 font-mono text-sm text-text"
                      {...props}
                    />
                  ),
                }}
              >
                {markdown}
              </ReactMarkdown>
            </article>
          </div>
        </motion.div>
      </motion.div>
    </AnimatePresence>
  );
}
