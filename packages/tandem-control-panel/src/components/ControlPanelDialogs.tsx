import { AnimatePresence, motion } from "motion/react";
import type { ReactNode } from "react";
import { useEffect, useRef } from "react";
import { Icon } from "../ui/Icon";

function useDialogIconRender(active: boolean) {
  const dialogRef = useRef<HTMLDivElement | null>(null);

  return dialogRef;
}

type DialogShellProps = {
  open: boolean;
  title: string;
  message?: any;
  widthClassName?: string;
  onCancel: () => void;
  children?: any;
  actions?: any;
};

function DialogShell({
  open,
  title,
  message,
  widthClassName = "w-[min(34rem,96vw)]",
  onCancel,
  children,
  actions,
}: DialogShellProps) {
  const dialogRef = useDialogIconRender(open);

  useEffect(() => {
    if (!open || typeof window === "undefined") return undefined;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onCancel();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, onCancel]);

  return (
    <AnimatePresence>
      {open ? (
        <motion.div
          className="tcp-confirm-overlay"
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0 }}
          onClick={onCancel}
        >
          <motion.div
            ref={dialogRef}
            className={`tcp-confirm-dialog ${widthClassName}`.trim()}
            initial={{ opacity: 0, y: 8, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 6, scale: 0.98 }}
            onClick={(event) => event.stopPropagation()}
          >
            <h3 className="tcp-confirm-title">{title}</h3>
            {message ? <div className="tcp-confirm-message">{message}</div> : null}
            {children}
            {actions ? <div className="tcp-confirm-actions mt-3">{actions}</div> : null}
          </motion.div>
        </motion.div>
      ) : null}
    </AnimatePresence>
  );
}

type ConfirmDialogProps = {
  open: boolean;
  title: string;
  message: any;
  children?: ReactNode;
  confirmLabel?: string;
  cancelLabel?: string;
  confirmIcon?: string;
  confirmTone?: "default" | "danger";
  confirmDisabled?: boolean;
  widthClassName?: string;
  onCancel: () => void;
  onConfirm: () => void;
};

export function ConfirmDialog({
  open,
  title,
  message,
  confirmLabel = "Confirm",
  cancelLabel = "Cancel",
  confirmIcon = "check",
  confirmTone = "danger",
  confirmDisabled = false,
  widthClassName,
  children,
  onCancel,
  onConfirm,
}: ConfirmDialogProps) {
  return (
    <DialogShell
      open={open}
      title={title}
      message={message}
      widthClassName={widthClassName}
      onCancel={onCancel}
      children={children}
      actions={
        <>
          <button type="button" className="tcp-btn" onClick={onCancel}>
            <Icon name="x" />
            {cancelLabel}
          </button>
          <button
            type="button"
            className={confirmTone === "danger" ? "tcp-btn-danger" : "tcp-btn-primary"}
            disabled={confirmDisabled}
            onClick={onConfirm}
          >
            <Icon name={confirmIcon} />
            {confirmLabel}
          </button>
        </>
      }
    />
  );
}

type PromptDialogProps = {
  open: boolean;
  title: string;
  message?: any;
  label?: string;
  value: string;
  placeholder?: string;
  confirmLabel?: string;
  cancelLabel?: string;
  confirmIcon?: string;
  confirmTone?: "default" | "primary" | "danger";
  confirmDisabled?: boolean;
  widthClassName?: string;
  autoFocus?: boolean;
  inputClassName?: string;
  onCancel: () => void;
  onChange: (value: string) => void;
  onConfirm: () => void;
};

export function PromptDialog({
  open,
  title,
  message,
  label = "Value",
  value,
  placeholder = "",
  confirmLabel = "Save",
  cancelLabel = "Cancel",
  confirmIcon = "check",
  confirmTone = "primary",
  confirmDisabled = false,
  widthClassName,
  autoFocus = true,
  inputClassName = "",
  onCancel,
  onChange,
  onConfirm,
}: PromptDialogProps) {
  const inputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    if (!open || !autoFocus) return;
    const input = inputRef.current;
    if (!input) return;
    window.setTimeout(() => {
      input.focus();
      input.select();
    }, 0);
  }, [autoFocus, open]);

  return (
    <DialogShell
      open={open}
      title={title}
      message={message}
      widthClassName={widthClassName}
      onCancel={onCancel}
      actions={
        <>
          <button type="button" className="tcp-btn" onClick={onCancel}>
            <Icon name="x" />
            {cancelLabel}
          </button>
          <button
            type="button"
            className={confirmTone === "danger" ? "tcp-btn-danger" : "tcp-btn-primary"}
            disabled={confirmDisabled}
            onClick={onConfirm}
          >
            <Icon name={confirmIcon} />
            {confirmLabel}
          </button>
        </>
      }
    >
      <label className="grid gap-1 text-left">
        <span className="text-xs uppercase tracking-wide text-slate-500">{label}</span>
        <input
          ref={inputRef}
          className={`tcp-input ${inputClassName}`.trim()}
          value={value}
          placeholder={placeholder}
          onChange={(event) => onChange(event.target.value)}
          onKeyDown={(event) => {
            if (event.key !== "Enter" || confirmDisabled) return;
            event.preventDefault();
            onConfirm();
          }}
        />
      </label>
    </DialogShell>
  );
}
