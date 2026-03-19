import { AnimatePresence, motion } from "motion/react";
import { EmptyState as PrimitiveEmptyState, PanelCard } from "../ui/index.tsx";

export function PageCard({
  title,
  subtitle,
  children,
  actions,
  className,
}: {
  title: any;
  subtitle?: string;
  children: any;
  actions?: any;
  className?: string;
}) {
  return (
    <PanelCard title={title} subtitle={subtitle} actions={actions} className={className || ""}>
      {children}
    </PanelCard>
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
      <AnimatePresence initial={false} mode="popLayout">
        {items.map((item, index) => (
          <motion.div
            key={String(item?.id ?? item?.key ?? index)}
            layout
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.16, ease: "easeOut" }}
          >
            {render(item, index)}
          </motion.div>
        ))}
      </AnimatePresence>
    </div>
  );
}

export function EmptyState({
  text,
  title,
  action,
}: {
  text: string;
  title?: string;
  action?: any;
}) {
  return <PrimitiveEmptyState text={text} title={title} action={action} />;
}

export function formatJson(value: unknown) {
  return JSON.stringify(value, null, 2);
}
