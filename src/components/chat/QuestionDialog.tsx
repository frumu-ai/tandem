import { useState } from "react";
import { motion, AnimatePresence } from "framer-motion";
import { MessageSquare, Check } from "lucide-react";
import { Button } from "@/components/ui";
import { cn } from "@/lib/utils";
import type { QuestionEvent } from "@/lib/tauri";

interface QuestionDialogProps {
  question: QuestionEvent | null;
  onAnswer: (answer: string) => void;
}

export function QuestionDialog({ question, onAnswer }: QuestionDialogProps) {
  const [selectedOption, setSelectedOption] = useState<string | null>(null);
  const [customAnswer, setCustomAnswer] = useState("");

  const handleSubmit = () => {
    const answer = selectedOption || customAnswer;
    if (answer) {
      onAnswer(answer);
      setSelectedOption(null);
      setCustomAnswer("");
    }
  };

  return (
    <AnimatePresence>
      {question && (
        <motion.div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/50 backdrop-blur-sm"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
        >
          <motion.div
            className="mx-4 w-full max-w-md rounded-xl border border-border bg-surface p-6 shadow-xl"
            initial={{ scale: 0.95, opacity: 0 }}
            animate={{ scale: 1, opacity: 1 }}
            exit={{ scale: 0.95, opacity: 0 }}
          >
            <div className="flex items-start gap-3">
              <MessageSquare className="h-6 w-6 text-primary flex-shrink-0 mt-0.5" />
              <div className="flex-1">
                {question.header && (
                  <h2 className="text-lg font-semibold text-text mb-2">{question.header}</h2>
                )}
                <p className="text-sm text-text mb-4">{question.question}</p>

                {/* Options */}
                <div className="space-y-2 mb-4">
                  {question.options.map((option) => (
                    <button
                      key={option.id}
                      onClick={() => setSelectedOption(option.label)}
                      className={cn(
                        "w-full text-left px-4 py-3 rounded-lg border transition-colors",
                        selectedOption === option.label
                          ? "border-primary bg-primary/10 text-text"
                          : "border-border bg-surface-elevated hover:border-primary/50 text-text"
                      )}
                    >
                      <div className="flex items-center justify-between">
                        <span>{option.label}</span>
                        {selectedOption === option.label && (
                          <Check className="h-4 w-4 text-primary" />
                        )}
                      </div>
                    </button>
                  ))}
                </div>

                {/* Custom answer */}
                <div className="mb-4">
                  <input
                    type="text"
                    placeholder="Or type a custom answer..."
                    value={customAnswer}
                    onChange={(e) => {
                      setCustomAnswer(e.target.value);
                      setSelectedOption(null);
                    }}
                    className="w-full px-3 py-2 rounded-lg border border-border bg-surface text-text placeholder:text-text-subtle focus:outline-none focus:border-primary"
                  />
                </div>

                <div className="flex justify-end">
                  <Button onClick={handleSubmit} disabled={!selectedOption && !customAnswer.trim()}>
                    Submit Answer
                  </Button>
                </div>
              </div>
            </div>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
