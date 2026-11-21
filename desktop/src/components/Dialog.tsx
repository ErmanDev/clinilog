import { Loader2, X } from "lucide-react";
import { useEffect, useId, useRef, type ReactNode } from "react";
import { buttonClass, ErrorNotice } from "./form";

/** Modal built on <dialog>: focus is trapped, Esc and the backdrop close it. */
export function Dialog({
  open,
  onClose,
  title,
  children,
  footer,
}: {
  open: boolean;
  onClose: () => void;
  title: string;
  children: ReactNode;
  footer?: ReactNode;
}) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();

  useEffect(() => {
    const d = ref.current;
    if (!d) return;
    if (open && !d.open) d.showModal();
    if (!open && d.open) d.close();
  }, [open]);

  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      onClose={onClose}
      onClick={(e) => {
        if (e.target === ref.current) onClose();
      }}
      className="m-auto w-full max-w-md rounded-xl p-0 shadow-xl"
    >
      {open && (
        <div className="p-6">
          <div className="flex items-start justify-between gap-4">
            <h2 id={titleId} className="text-lg font-semibold">
              {title}
            </h2>
            <button
              type="button"
              onClick={onClose}
              className="rounded-md p-1 text-slate-500 hover:bg-slate-100 hover:text-slate-700"
              aria-label="Close"
            >
              <X className="size-5" aria-hidden />
            </button>
          </div>
          <div className="mt-3 text-sm text-slate-700">{children}</div>
          {footer && <div className="mt-6 flex justify-end gap-2">{footer}</div>}
        </div>
      )}
    </dialog>
  );
}

/** "Are you sure?" dialog. Shows `error` inline if the action fails. */
export function ConfirmDialog({
  open,
  title,
  children,
  confirmLabel,
  tone = "danger",
  busy,
  error,
  onConfirm,
  onCancel,
}: {
  open: boolean;
  title: string;
  children: ReactNode;
  confirmLabel: string;
  tone?: "danger" | "primary";
  busy?: boolean;
  error?: string | null;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Dialog
      open={open}
      onClose={() => !busy && onCancel()}
      title={title}
      footer={
        <>
          <button type="button" className={buttonClass("secondary")} onClick={onCancel} disabled={busy} autoFocus>
            Cancel
          </button>
          <button type="button" className={buttonClass(tone)} onClick={onConfirm} disabled={busy}>
            {busy && <Loader2 className="size-4 animate-spin" aria-hidden />}
            {confirmLabel}
          </button>
        </>
      }
    >
      <div className="space-y-3">
        {children}
        {error && <ErrorNotice>{error}</ErrorNotice>}
      </div>
    </Dialog>
  );
}
