import { AnimatePresence, motion } from "motion/react";

export function PageCard({
  title,
  subtitle,
  children,
  actions,
}: {
  title: string;
  subtitle?: string;
  children: any;
  actions?: any;
}) {
  return (
    <motion.section
      className="tcp-card"
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ duration: 0.14, ease: "easeOut" }}
    >
      <div className="mb-3 flex items-start justify-between gap-3">
        <div>
          <h3 className="tcp-title">{title}</h3>
          {subtitle ? <p className="tcp-subtle mt-1">{subtitle}</p> : null}
        </div>
        {actions || null}
      </div>
      {children}
    </motion.section>
  );
}

export function AnimatedList({
  items,
  render,
}: {
  items: any[];
  render: (item: any, index: number) => any;
}) {
  return (
    <div className="grid gap-2">
      <AnimatePresence initial={false}>
        {items.map((item, index) => (
          <motion.div
            key={String(item?.id ?? item?.key ?? index)}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ duration: 0.12, ease: "easeOut" }}
          >
            {render(item, index)}
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}

export function EmptyState({ text }: { text: string }) {
  return <p className="tcp-subtle rounded-xl border border-slate-700/50 bg-black/20 p-3">{text}</p>;
}

export function formatJson(value: unknown) {
  return JSON.stringify(value, null, 2);
}
