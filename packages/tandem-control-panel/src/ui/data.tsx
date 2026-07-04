import { useEffect, useRef, useState } from "react";
import { Icon } from "./Icon";

// Shared data-display primitives. These replace the per-page reimplementations
// of status tones, relative timestamps, copyable IDs, and key/value rows that
// had drifted apart across the app (see TAN-586).

export type StatusTone = "ok" | "warn" | "err" | "info" | "blocked" | "neutral";

const STATUS_TONE_BADGE: Record<StatusTone, string> = {
  ok: "tcp-badge-ok",
  warn: "tcp-badge-warn",
  err: "tcp-badge-err",
  info: "tcp-badge-info",
  blocked: "tcp-badge-blocked",
  neutral: "tcp-badge tcp-badge-ghost",
};

/** Map an arbitrary backend status string to one of the shared badge tones. */
export function statusTone(status: string | null | undefined): StatusTone {
  const s = String(status || "").toLowerCase().trim();
  if (!s) return "neutral";
  if (
    ["ok", "active", "ready", "completed", "complete", "done", "posted", "succeeded", "success", "healthy", "enabled", "verified", "resolved"].includes(s)
  ) {
    return "ok";
  }
  if (["running", "in_progress", "in-progress", "pending", "queued", "waiting", "processing", "checking"].includes(s)) {
    return "warn";
  }
  if (["failed", "error", "stalled", "denied", "rejected", "cancelled", "canceled", "unavailable", "disabled"].includes(s)) {
    return "err";
  }
  if (["blocked", "quarantined", "held", "paused", "suspended"].includes(s)) {
    return "blocked";
  }
  return "info";
}

/** "in_progress" / "MONITORING_ACTIVE" -> "In progress" / "Monitoring active". */
export function humanizeLabel(value: string | null | undefined): string {
  const s = String(value || "").replace(/[_-]+/g, " ").trim();
  if (!s) return "";
  const lower = s.toLowerCase();
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}

export function StatusBadge({
  status,
  label,
  className = "",
}: {
  status: string | null | undefined;
  label?: string;
  className?: string;
}) {
  const tone = statusTone(status);
  return (
    <span className={`${STATUS_TONE_BADGE[tone]} ${className}`.trim()}>
      {label ?? (humanizeLabel(status) || "—")}
    </span>
  );
}

function formatRelative(ms: number, nowMs: number): string {
  const diff = nowMs - ms;
  const abs = Math.abs(diff);
  const suffix = diff >= 0 ? "ago" : "from now";
  const sec = Math.round(abs / 1000);
  if (sec < 45) return "just now";
  const min = Math.round(sec / 60);
  if (min < 60) return `${min}m ${suffix}`;
  const hr = Math.round(min / 60);
  if (hr < 24) return `${hr}h ${suffix}`;
  const day = Math.round(hr / 24);
  if (day < 30) return `${day}d ${suffix}`;
  const mon = Math.round(day / 30);
  if (mon < 12) return `${mon}mo ${suffix}`;
  return `${Math.round(mon / 12)}y ${suffix}`;
}

/** Relative time ("2h ago") with the absolute timestamp on hover. */
export function RelativeTime({
  ms,
  className = "",
}: {
  ms: number | null | undefined;
  className?: string;
}) {
  const value = Number(ms);
  const [now, setNow] = useState<number | null>(null);
  useEffect(() => {
    setNow(Date.now());
    const timer = setInterval(() => setNow(Date.now()), 60_000);
    return () => clearInterval(timer);
  }, []);
  if (!Number.isFinite(value) || value <= 0) {
    return <span className={className}>n/a</span>;
  }
  const absolute = new Date(value).toLocaleString();
  // Render the absolute string until mounted so SSR/first paint is stable.
  return (
    <span className={className} title={absolute}>
      {now == null ? absolute : formatRelative(value, now)}
    </span>
  );
}

async function copyToClipboard(text: string): Promise<boolean> {
  try {
    if (navigator?.clipboard?.writeText) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // fall through to legacy path
  }
  try {
    const area = document.createElement("textarea");
    area.value = text;
    area.style.position = "fixed";
    area.style.opacity = "0";
    document.body.appendChild(area);
    area.select();
    const ok = document.execCommand("copy");
    document.body.removeChild(area);
    return ok;
  } catch {
    return false;
  }
}

/** Small icon button that copies `value` and briefly shows a check. */
export function CopyButton({
  value,
  label = "Copy",
  className = "",
}: {
  value: string;
  label?: string;
  className?: string;
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);
  return (
    <button
      type="button"
      className={`tcp-icon-btn h-6 w-6 ${className}`.trim()}
      title={copied ? "Copied" : label}
      aria-label={copied ? "Copied" : label}
      onClick={async (event) => {
        event.stopPropagation();
        if (await copyToClipboard(value)) {
          setCopied(true);
          if (timer.current) clearTimeout(timer.current);
          timer.current = setTimeout(() => setCopied(false), 1200);
        }
      }}
    >
      <Icon name={copied ? "check" : "copy"} />
    </button>
  );
}

/** Monospace, middle-truncated identifier with a click-to-copy affordance. */
export function IdChip({
  value,
  className = "",
  head = 6,
  tail = 4,
}: {
  value: string | null | undefined;
  className?: string;
  head?: number;
  tail?: number;
}) {
  const id = String(value || "").trim();
  if (!id) return <span className="tcp-subtle text-xs">—</span>;
  const shortened =
    id.length > head + tail + 1 ? `${id.slice(0, head)}…${id.slice(-tail)}` : id;
  return (
    <span className={`inline-flex items-center gap-1 ${className}`.trim()}>
      <code className="font-mono tcp-text-caption text-tcp-text-tertiary" title={id}>
        {shortened}
      </code>
      <CopyButton value={id} label="Copy ID" />
    </span>
  );
}

/** A single label/value row for definition-list style detail blocks. */
export function KeyValueRow({
  label,
  value,
  className = "",
}: {
  label: any;
  value: any;
  className?: string;
}) {
  return (
    <div className={`flex items-baseline justify-between gap-3 py-1 ${className}`.trim()}>
      <span className="tcp-subtle shrink-0 tcp-text-caption uppercase tracking-[0.14em]">{label}</span>
      <span className="min-w-0 break-words text-right text-xs text-tcp-text-secondary">{value}</span>
    </div>
  );
}
