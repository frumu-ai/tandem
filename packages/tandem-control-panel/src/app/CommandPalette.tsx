import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useState } from "react";

export type PaletteAction = {
  id: string;
  label: string;
  group: string;
  onSelect: () => void;
};

export function usePaletteHotkey(onToggle: () => void) {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        onToggle();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onToggle]);
}

export function CommandPalette({
  open,
  onClose,
  actions,
}: {
  open: boolean;
  onClose: () => void;
  actions: PaletteAction[];
}) {
  const [query, setQuery] = useState("");

  useEffect(() => {
    if (!open) setQuery("");
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onClose]);

  const filtered = useMemo(() => {
    const term = query.trim().toLowerCase();
    if (!term) return actions;
    return actions.filter((action) =>
      `${action.label} ${action.group}`.toLowerCase().includes(term)
    );
  }, [actions, query]);

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          className="tcp-confirm-overlay"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onClick={onClose}
        >
          <motion.div
            className="tcp-doc-dialog w-[min(44rem,96vw)]"
            initial={{ opacity: 0, y: 8, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 6, scale: 0.98 }}
            transition={{ duration: 0.16, ease: "easeOut" }}
            onClick={(e) => e.stopPropagation()}
          >
            <div className="tcp-doc-header">
              <h3 className="tcp-doc-title">Command Palette</h3>
              <kbd className="tcp-subtle text-xs">Ctrl/Cmd + K</kbd>
            </div>
            <div className="p-3">
              <input
                autoFocus
                value={query}
                onInput={(e) => setQuery((e.target as HTMLInputElement).value)}
                placeholder="Jump to route or action"
                className="tcp-input"
              />
              <div className="mt-3 grid max-h-[50vh] gap-2 overflow-auto">
                {filtered.map((action) => (
                  <button
                    key={action.id}
                    type="button"
                    className="tcp-btn w-full justify-between"
                    onClick={() => {
                      action.onSelect();
                      onClose();
                    }}
                  >
                    <span>{action.label}</span>
                    <span className="tcp-subtle text-xs">{action.group}</span>
                  </button>
                ))}
                {!filtered.length ? (
                  <p className="tcp-subtle px-1 py-2">No matching action.</p>
                ) : null}
              </div>
            </div>
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}
