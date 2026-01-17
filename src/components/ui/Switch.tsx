import { forwardRef, type InputHTMLAttributes } from "react";
import { cn } from "@/lib/utils";
import { motion } from "framer-motion";

export interface SwitchProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "type"> {
  label?: string;
}

const Switch = forwardRef<HTMLInputElement, SwitchProps>(
  ({ className, label, checked, onChange, disabled, ...props }, ref) => {
    return (
      <label className={cn("inline-flex cursor-pointer items-center gap-3", disabled && "cursor-not-allowed opacity-50", className)}>
        <div className="relative">
          <input
            type="checkbox"
            className="sr-only"
            ref={ref}
            checked={checked}
            onChange={onChange}
            disabled={disabled}
            {...props}
          />
          <motion.div
            className={cn(
              "h-6 w-11 rounded-full transition-colors duration-200",
              checked ? "bg-primary" : "bg-border"
            )}
          >
            <motion.div
              className="absolute left-0.5 top-0.5 h-5 w-5 rounded-full bg-white shadow-sm"
              animate={{ x: checked ? 20 : 0 }}
              transition={{ type: "spring", stiffness: 500, damping: 30 }}
            />
          </motion.div>
        </div>
        {label && <span className="text-sm text-text-muted">{label}</span>}
      </label>
    );
  }
);

Switch.displayName = "Switch";

export { Switch };
