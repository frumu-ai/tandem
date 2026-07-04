import { motion } from "motion/react";
import { TandemLogoAnimation } from "../../ui/TandemLogoAnimation";

export function PlanGenerationAnimation() {
  return (
    <div className="relative flex aspect-square w-full items-center justify-center overflow-hidden rounded-none border border-white/5 bg-black/40">
      <div className="absolute inset-0 opacity-20">
        <div className="tcp-shell-glow tcp-shell-glow-a opacity-50" />
        <div className="tcp-shell-glow tcp-shell-glow-b opacity-30" />
      </div>

      <div className="relative z-10 grid w-full gap-5 px-6 py-6">
        <TandemLogoAnimation className="mx-auto aspect-square w-full max-w-[17rem]" mode="panel" />
        <div className="grid gap-2">
          {[
            "Mapping objective into shared workflow lanes",
            "Scoring schedule and workspace constraints",
            "Composing a mission draft with recovery steps",
          ].map((label, index) => (
            <div key={label} className="grid gap-1 border border-white/5 bg-black/35 px-3 py-2">
              <div className="flex items-center justify-between gap-3">
                <span className="tcp-text-caption font-medium uppercase tracking-[0.22em] text-slate-300">
                  {label}
                </span>
                <span className="tcp-text-micro uppercase tracking-[0.22em] text-amber-200/75">
                  Live
                </span>
              </div>
              <motion.div
                className="h-1.5 origin-left bg-[linear-gradient(90deg,rgba(245,158,11,0.18),rgba(255,233,171,0.94),rgba(245,158,11,0.18))]"
                animate={{ scaleX: [0.22, 0.94, 0.38] }}
                transition={{
                  duration: 1.8,
                  ease: "easeInOut",
                  repeat: Infinity,
                  delay: index * 0.22,
                }}
              />
            </div>
          ))}
        </div>
      </div>

      <div className="absolute inset-x-0 bottom-6 text-center">
        <div className="inline-flex items-center gap-2 rounded-none border border-primary/20 bg-black/60 px-4 py-2 backdrop-blur-md">
          <span className="h-2 w-2 animate-pulse rounded-full bg-primary" />
          <span className="font-display text-xs font-semibold uppercase tracking-widest text-primary">
            Synthesizing Plan Flow
          </span>
        </div>
      </div>
    </div>
  );
}
