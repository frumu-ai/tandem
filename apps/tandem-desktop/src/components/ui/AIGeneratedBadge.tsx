import { Sparkles } from "lucide-react";

const TRANSPARENCY_TOOLTIP =
  "This content was produced or materially transformed by an AI system. Review before relying on it.";

type AIGeneratedState = "draft" | "reviewed" | "approved" | "assisted";

type AIGeneratedBadgeProps = {
  state?: AIGeneratedState;
  className?: string;
};

const STATE_CONFIG: Record<AIGeneratedState, { label: string; chip: string; ariaLabel: string }> = {
  draft: {
    label: "AI-Generated",
    chip: "border-primary/40 bg-primary/10 text-primary",
    ariaLabel: `AI-Generated draft. ${TRANSPARENCY_TOOLTIP}`,
  },
  reviewed: {
    label: "AI-Generated, reviewed",
    chip: "border-amber-400/40 bg-amber-400/10 text-amber-200",
    ariaLabel: `AI-Generated, reviewed by a human. ${TRANSPARENCY_TOOLTIP}`,
  },
  approved: {
    label: "AI-Generated, approved",
    chip: "border-emerald-500/40 bg-emerald-500/10 text-emerald-200",
    ariaLabel: `AI-Generated content with recorded human approval. ${TRANSPARENCY_TOOLTIP}`,
  },
  assisted: {
    label: "AI-Assisted",
    chip: "border-primary/30 bg-primary/5 text-primary/80",
    ariaLabel: `AI-Assisted. ${TRANSPARENCY_TOOLTIP}`,
  },
};

/**
 * Article 50 EU AI Act transparency label for AI-generated and AI-assisted content.
 * Place this badge close to generated text, plans, artifacts, and summaries.
 */
export function AIGeneratedBadge({ state = "draft", className = "" }: AIGeneratedBadgeProps) {
  const config = STATE_CONFIG[state];
  return (
    <span
      className={`inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-medium ${config.chip} ${className}`}
      title={TRANSPARENCY_TOOLTIP}
      aria-label={config.ariaLabel}
      role="img"
    >
      <Sparkles className="h-2.5 w-2.5" aria-hidden />
      {config.label}
    </span>
  );
}
