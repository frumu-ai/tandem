import { motion, AnimatePresence } from "framer-motion";
import { Check, MessageSquare } from "lucide-react";
import { useMemo, useState } from "react";
import { Button } from "@/components/ui";
import { cn } from "@/lib/utils";
import type { QuestionRequestEvent } from "@/lib/tauri";

interface QuestionDialogProps {
  request: QuestionRequestEvent | null;
  onSubmit: (answers: string[][]) => void;
  onReject: () => void;
  canViewPlan?: boolean;
  onViewPlan?: () => void;
  planLabel?: string;
}

type DraftAnswer = {
  selected: string[];
  custom: string;
};

export function QuestionDialog({
  request,
  onSubmit,
  onReject,
  canViewPlan = false,
  onViewPlan,
  planLabel,
}: QuestionDialogProps) {
  const [questionIndex, setQuestionIndex] = useState(0);
  const [draftAnswers, setDraftAnswers] = useState<DraftAnswer[]>(() =>
    request ? request.questions.map(() => ({ selected: [], custom: "" })) : []
  );

  const currentQuestion = request?.questions[questionIndex];
  const currentDraft = draftAnswers[questionIndex];

  const allowMultiple = currentQuestion?.multiple ?? false;
  const allowCustom = currentQuestion?.custom !== false;

  const hasAnswer = useMemo(() => {
    if (!request || !currentQuestion || !currentDraft) return false;
    return currentDraft.selected.length > 0 || currentDraft.custom.trim().length > 0;
  }, [request, currentQuestion, currentDraft]);

  const setCurrentDraft = (updater: (prev: DraftAnswer) => DraftAnswer) => {
    setDraftAnswers((prev) => {
      if (!request) return prev;
      const next = [...prev];
      const existing = next[questionIndex] || { selected: [], custom: "" };
      next[questionIndex] = updater(existing);
      return next;
    });
  };

  const toggleOption = (label: string) => {
    if (!currentQuestion) return;

    if (allowMultiple) {
      setCurrentDraft((prev) => {
        const exists = prev.selected.includes(label);
        return {
          ...prev,
          selected: exists ? prev.selected.filter((x) => x !== label) : [...prev.selected, label],
        };
      });
      return;
    }

    setCurrentDraft((_prev) => ({ selected: [label], custom: "" }));
  };

  const updateCustom = (value: string) => {
    if (!currentQuestion) return;
    setCurrentDraft((prev) => ({
      selected: allowMultiple ? prev.selected : [],
      custom: value,
    }));
  };

  const buildFinalAnswers = (): string[][] => {
    if (!request) return [];

    return request.questions.map((q, i) => {
      const draft = draftAnswers[i] || { selected: [], custom: "" };
      const custom = draft.custom.trim();
      const out = [...draft.selected];

      if (custom && !out.includes(custom) && q.custom !== false) {
        out.push(custom);
      }

      return out;
    });
  };

  const handleNext = () => {
    if (!request) return;
    if (questionIndex < request.questions.length - 1) {
      setQuestionIndex((i) => i + 1);
      return;
    }
    onSubmit(buildFinalAnswers());
  };

  const handleBack = () => {
    setQuestionIndex((i) => Math.max(0, i - 1));
  };

  return (
    <AnimatePresence>
      {request && currentQuestion && currentDraft && (
        <motion.div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/25"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
        >
          <motion.div
            className="mx-4 w-full max-w-lg rounded-xl border border-border bg-surface p-6 shadow-xl"
            initial={{ scale: 0.95, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.95, opacity: 0 }}
          >
            <div className="flex items-start gap-3">
              <MessageSquare className="h-6 w-6 text-primary flex-shrink-0 mt-0.5" />
              <div className="flex-1">
                <div className="flex items-baseline justify-between gap-3 mb-2">
                  <h2 className="text-lg font-semibold text-text">
                    {currentQuestion.header || "Question"}
                  </h2>
                  <div className="text-xs text-text-muted">
                    {questionIndex + 1} / {request.questions.length}
                  </div>
                </div>
                <p className="text-sm text-text mb-4">{currentQuestion.question}</p>

                {canViewPlan && (
                  <div className="mb-4 rounded-lg border border-primary/30 bg-primary/10 p-3">
                    <div className="flex items-center justify-between gap-3">
                      <div className="min-w-0">
                        <div className="text-xs font-medium text-primary">Plan available</div>
                        <div className="truncate text-xs text-text-muted">
                          {planLabel || "Open the latest plan for review"}
                        </div>
                      </div>
                      <Button
                        variant="secondary"
                        className="h-8 px-3 text-xs"
                        onClick={onViewPlan}
                        type="button"
                      >
                        View plan
                      </Button>
                    </div>
                  </div>
                )}

                {/* Options */}
                <div className="space-y-2 mb-4">
                  {currentQuestion.options.map((option) => {
                    const isSelected = currentDraft.selected.includes(option.label);
                    return (
                      <button
                        key={option.label}
                        onClick={() => toggleOption(option.label)}
                        className={cn(
                          "w-full text-left px-4 py-3 rounded-lg border transition-colors",
                          isSelected
                            ? "border-primary bg-primary/10 text-text"
                            : "border-border bg-surface-elevated hover:border-primary/50 text-text"
                        )}
                      >
                        <div className="flex items-start justify-between gap-3">
                          <div className="min-w-0">
                            <div className="font-medium">{option.label}</div>
                            {option.description && (
                              <div className="text-xs text-text-muted mt-0.5">
                                {option.description}
                              </div>
                            )}
                          </div>
                          {isSelected && <Check className="h-4 w-4 text-primary flex-shrink-0" />}
                        </div>
                      </button>
                    );
                  })}
                </div>

                {/* Custom answer */}
                {allowCustom && (
                  <div className="mb-4">
                    <input
                      type="text"
                      placeholder={
                        allowMultiple ? "Add a custom answer..." : "Or type a custom answer..."
                      }
                      value={currentDraft.custom}
                      onChange={(e) => updateCustom(e.target.value)}
                      className="w-full px-3 py-2 rounded-lg border border-border bg-surface text-text placeholder:text-text-subtle focus:outline-none focus:border-primary"
                    />
                  </div>
                )}

                <div className="flex items-center justify-between gap-3">
                  <Button variant="ghost" onClick={onReject}>
                    Cancel
                  </Button>

                  <div className="flex items-center gap-2">
                    <Button variant="secondary" onClick={handleBack} disabled={questionIndex === 0}>
                      Back
                    </Button>
                    <Button onClick={handleNext} disabled={!hasAnswer}>
                      {questionIndex < request.questions.length - 1 ? "Next" : "Submit"}
                    </Button>
                  </div>
                </div>
              </div>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
