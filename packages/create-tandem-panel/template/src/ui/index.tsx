import { AnimatePresence, motion } from "motion/react";
import { useEffect, useMemo, useRef, useState } from "react";
import { MOTION_TOKENS, prefersReducedMotion } from "../app/themes.js";

function useReducedMotionPreference() {
  const [reduced, setReduced] = useState(() => prefersReducedMotion());

  useEffect(() => {
    if (typeof window === "undefined" || typeof window.matchMedia !== "function") return undefined;
    const media = window.matchMedia("(prefers-reduced-motion: reduce)");
    const onChange = () => setReduced(media.matches);
    onChange();
    if (typeof media.addEventListener === "function") {
      media.addEventListener("change", onChange);
      return () => media.removeEventListener("change", onChange);
    }
    media.addListener(onChange);
    return () => media.removeListener(onChange);
  }, []);

  return reduced;
}

function clampNumber(value, min, max) {
  return Math.min(max, Math.max(min, value));
}

export function GlowLayer({ className = "", children }: { className?: string; children?: any }) {
  return <div className={`tcp-glow-layer ${className}`.trim()}>{children}</div>;
}

export function AnimatedPage({ className = "", children }: { className?: string; children?: any }) {
  const reducedMotion = useReducedMotionPreference();
  return (
    <motion.div
      className={className}
      initial={reducedMotion ? false : { opacity: 0, y: 14 }}
      animate={reducedMotion ? undefined : { opacity: 1, y: 0 }}
      exit={reducedMotion ? undefined : { opacity: 0, y: -10 }}
      transition={
        reducedMotion
          ? undefined
          : {
              duration: MOTION_TOKENS.duration.normal,
              ease: MOTION_TOKENS.easing.standard,
            }
      }
    >
      {children}
    </motion.div>
  );
}

export function AnimatedPresenceStack({ children }: { children: any }) {
  return <AnimatePresence mode="popLayout">{children}</AnimatePresence>;
}

export function StaggerGroup({ className = "", children }: { className?: string; children: any }) {
  const reducedMotion = useReducedMotionPreference();
  return (
    <motion.div
      className={className}
      initial={reducedMotion ? false : "hidden"}
      animate={reducedMotion ? undefined : "show"}
      variants={
        reducedMotion
          ? undefined
          : {
              hidden: {},
              show: {
                transition: {
                  staggerChildren: 0.06,
                  delayChildren: 0.02,
                },
              },
            }
      }
    >
      {children}
    </motion.div>
  );
}

export function RevealCard({
  className = "",
  children,
  as = "div",
}: {
  className?: string;
  children: any;
  as?: "div" | "section" | "article";
}) {
  const reducedMotion = useReducedMotionPreference();
  const Comp = as === "section" ? motion.section : as === "article" ? motion.article : motion.div;
  return (
    <Comp
      className={className}
      variants={
        reducedMotion
          ? undefined
          : {
              hidden: { opacity: 0, y: 12, scale: 0.985 },
              show: {
                opacity: 1,
                y: 0,
                scale: 1,
                transition: {
                  duration: MOTION_TOKENS.duration.normal,
                  ease: MOTION_TOKENS.easing.standard,
                },
              },
            }
      }
      initial={reducedMotion ? false : "hidden"}
      animate={reducedMotion ? undefined : "show"}
    >
      {children}
    </Comp>
  );
}

export function MotionNumber({
  value,
  format = (next) => String(next),
  className = "",
}: {
  value: number;
  format?: (value: number) => string;
  className?: string;
}) {
  const reducedMotion = useReducedMotionPreference();
  const [display, setDisplay] = useState(() => Number(value || 0));
  const previousValue = useRef(Number(value || 0));

  useEffect(() => {
    const next = Number(value || 0);
    if (!Number.isFinite(next)) {
      setDisplay(0);
      previousValue.current = 0;
      return undefined;
    }
    if (reducedMotion) {
      setDisplay(next);
      previousValue.current = next;
      return undefined;
    }
    const start = previousValue.current;
    const diff = next - start;
    if (!diff) {
      setDisplay(next);
      previousValue.current = next;
      return undefined;
    }
    const startedAt = performance.now();
    let raf = 0;
    const tick = (ts) => {
      const progress = clampNumber((ts - startedAt) / 420, 0, 1);
      const eased = 1 - Math.pow(1 - progress, 3);
      const frame = start + diff * eased;
      setDisplay(frame);
      if (progress < 1) {
        raf = window.requestAnimationFrame(tick);
      } else {
        previousValue.current = next;
      }
    };
    raf = window.requestAnimationFrame(tick);
    return () => window.cancelAnimationFrame(raf);
  }, [reducedMotion, value]);

  return <span className={className}>{format(Number.isFinite(display) ? display : 0)}</span>;
}

export function StatusPulse({
  tone = "ok",
  text,
  className = "",
}: {
  tone?: "ok" | "warn" | "live" | "info";
  text?: string;
  className?: string;
}) {
  return (
    <span className={`tcp-status-pulse ${tone} ${className}`.trim()}>
      <span className="tcp-status-pulse-dot" aria-hidden="true"></span>
      {text ? <span>{text}</span> : null}
    </span>
  );
}

export function Toolbar({ className = "", children }: { className?: string; children: any }) {
  return <div className={`tcp-toolbar ${className}`.trim()}>{children}</div>;
}

export function IconButton({
  className = "",
  title,
  children,
  ...props
}: {
  className?: string;
  title?: string;
  children?: any;
  [key: string]: any;
}) {
  return (
    <button type="button" title={title} className={`tcp-icon-btn ${className}`.trim()} {...props}>
      {children}
    </button>
  );
}

export function Badge({
  tone = "info",
  className = "",
  children,
}: {
  tone?: "ok" | "warn" | "err" | "info" | "ghost";
  className?: string;
  children: any;
}) {
  const toneClass =
    tone === "ok"
      ? "tcp-badge-ok"
      : tone === "warn"
        ? "tcp-badge-warn"
        : tone === "err"
          ? "tcp-badge-err"
          : tone === "ghost"
            ? "tcp-badge tcp-badge-ghost"
            : "tcp-badge-info";
  return <span className={`${toneClass} ${className}`.trim()}>{children}</span>;
}

export function FilterChip({
  active = false,
  className = "",
  children,
  ...props
}: {
  active?: boolean;
  className?: string;
  children: any;
  [key: string]: any;
}) {
  return (
    <button
      type="button"
      className={`tcp-filter-chip ${active ? "active" : ""} ${className}`.trim()}
      {...props}
    >
      {children}
    </button>
  );
}

export function PanelCard({
  title,
  subtitle,
  actions,
  className = "",
  children,
}: {
  title?: any;
  subtitle?: any;
  actions?: any;
  className?: string;
  children: any;
}) {
  return (
    <RevealCard as="section" className={`tcp-panel-card ${className}`.trim()}>
      {title || subtitle || actions ? (
        <div className="tcp-panel-card-head">
          <div className="min-w-0">
            {title ? <h3 className="tcp-title">{title}</h3> : null}
            {subtitle ? <p className="tcp-subtle mt-1">{subtitle}</p> : null}
          </div>
          {actions ? <div className="shrink-0">{actions}</div> : null}
        </div>
      ) : null}
      {children}
    </RevealCard>
  );
}

export function SectionCard(props: any) {
  return <PanelCard {...props} />;
}

export function EmptyState({
  text,
  title = "Nothing here yet",
  action,
  className = "",
}: {
  text: string;
  title?: string;
  action?: any;
  className?: string;
}) {
  return (
    <div className={`tcp-empty-state ${className}`.trim()}>
      <div className="tcp-empty-state-orb"></div>
      <div className="relative z-10">
        <div className="tcp-empty-state-title">{title}</div>
        <p className="tcp-subtle mt-1">{text}</p>
        {action ? <div className="mt-3">{action}</div> : null}
      </div>
    </div>
  );
}

export function PageHeader({
  eyebrow,
  title,
  subtitle,
  badges,
  actions,
  className = "",
}: {
  eyebrow?: string;
  title: any;
  subtitle?: any;
  badges?: any;
  actions?: any;
  className?: string;
}) {
  return (
    <RevealCard className={`tcp-page-header ${className}`.trim()}>
      <GlowLayer className="tcp-page-header-glow" />
      <div className="relative z-10 flex flex-col gap-4 lg:flex-row lg:items-end lg:justify-between">
        <div className="min-w-0">
          {eyebrow ? <div className="tcp-page-eyebrow">{eyebrow}</div> : null}
          <h1 className="tcp-page-title">{title}</h1>
          {subtitle ? <p className="tcp-subtle mt-2 max-w-3xl">{subtitle}</p> : null}
          {badges ? <div className="mt-3 flex flex-wrap gap-2">{badges}</div> : null}
        </div>
        {actions ? <Toolbar className="justify-start lg:justify-end">{actions}</Toolbar> : null}
      </div>
    </RevealCard>
  );
}

export function SplitView({
  className = "",
  mainClassName = "",
  asideClassName = "",
  main,
  aside,
}: {
  className?: string;
  mainClassName?: string;
  asideClassName?: string;
  main: any;
  aside?: any;
}) {
  return (
    <div className={`tcp-split-view ${className}`.trim()}>
      <div className={`min-w-0 ${mainClassName}`.trim()}>{main}</div>
      {aside ? <div className={`min-w-0 ${asideClassName}`.trim()}>{aside}</div> : null}
    </div>
  );
}

export function DetailDrawer({
  open,
  title,
  onClose,
  children,
}: {
  open: boolean;
  title?: any;
  onClose: () => void;
  children: any;
}) {
  const reducedMotion = useReducedMotionPreference();
  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          className="tcp-drawer-root"
          initial={reducedMotion ? false : { opacity: 0 }}
          animate={reducedMotion ? undefined : { opacity: 1 }}
          exit={reducedMotion ? undefined : { opacity: 0 }}
        >
          <button
            type="button"
            className="tcp-drawer-backdrop"
            aria-label="Close"
            onClick={onClose}
          />
          <motion.aside
            className="tcp-drawer-panel"
            initial={reducedMotion ? false : { x: "100%" }}
            animate={reducedMotion ? undefined : { x: 0 }}
            exit={reducedMotion ? undefined : { x: "100%" }}
            transition={reducedMotion ? undefined : MOTION_TOKENS.spring.drawer}
          >
            <div className="tcp-drawer-head">
              <div className="min-w-0">{title ? <h3 className="tcp-title">{title}</h3> : null}</div>
              <IconButton title="Close drawer" onClick={onClose}>
                <i data-lucide="x"></i>
              </IconButton>
            </div>
            <div className="min-h-0 flex-1 overflow-auto p-4">{children}</div>
          </motion.aside>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}

export function useThemePreview(themes: any[], themeId: string) {
  return useMemo(
    () => themes.find((theme) => theme.id === themeId) || themes[0] || null,
    [themeId, themes]
  );
}
