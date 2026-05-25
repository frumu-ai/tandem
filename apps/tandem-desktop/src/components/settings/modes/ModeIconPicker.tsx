import { useEffect, useMemo, useRef, useState } from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "@/lib/utils";
import { MODE_ICON_OPTIONS, getModeIconOptionById } from "./modeIcons";

interface ModeIconPickerProps {
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
}

export function ModeIconPicker({ value, onChange, disabled = false }: ModeIconPickerProps) {
  const [isOpen, setIsOpen] = useState(false);
  const containerRef = useRef<HTMLDivElement>(null);

  const selected = useMemo(() => getModeIconOptionById(value) ?? MODE_ICON_OPTIONS[0], [value]);
  const SelectedIcon = selected.icon;

  useEffect(() => {
    // eslint-disable-next-line no-undef
    const onClickOutside = (event: Event) => {
      if (!containerRef.current) return;
      // eslint-disable-next-line no-undef
      if (!containerRef.current.contains(event.target as Node)) {
        setIsOpen(false);
      }
    };

    if (isOpen) {
      document.addEventListener("mousedown", onClickOutside);
      return () => document.removeEventListener("mousedown", onClickOutside);
    }
  }, [isOpen]);

  return (
    <div ref={containerRef} className="relative">
      <button
        type="button"
        disabled={disabled}
        onClick={() => setIsOpen((prev) => !prev)}
        className={cn(
          "mt-1 flex w-full items-center justify-between rounded-md border border-border bg-surface px-3 py-2 text-sm text-text",
          disabled && "cursor-not-allowed opacity-60"
        )}
      >
        <span className="flex items-center gap-2">
          <SelectedIcon className="h-4 w-4" />
          <span>{selected.label}</span>
        </span>
        <ChevronDown
          className={cn("h-4 w-4 text-text-muted transition-transform", isOpen && "rotate-180")}
        />
      </button>

      {isOpen && (
        <div className="absolute z-50 mt-1 max-h-64 w-full overflow-y-auto rounded-md border border-border bg-surface-elevated shadow-lg">
          <div className="p-1">
            {MODE_ICON_OPTIONS.map((option) => {
              const OptionIcon = option.icon;
              const isSelected = option.id === selected.id;
              return (
                <button
                  key={option.id}
                  type="button"
                  onClick={() => {
                    onChange(option.id);
                    setIsOpen(false);
                  }}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm transition-colors",
                    isSelected
                      ? "bg-primary/10 text-primary"
                      : "text-text hover:bg-surface hover:text-text"
                  )}
                >
                  <OptionIcon className="h-4 w-4" />
                  <span>{option.label}</span>
                </button>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}
