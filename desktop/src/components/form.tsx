import { AlertCircle } from "lucide-react";
import { useId, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes } from "react";

export function inputClass(invalid: boolean, extra = "") {
  return `mt-1 block w-full rounded-lg border bg-white px-3 py-2 text-sm shadow-sm outline-none focus:ring-2 disabled:bg-slate-50 disabled:text-slate-500 ${
    invalid ? "border-red-400 focus:ring-red-200" : "border-slate-300 focus:border-teal-600 focus:ring-teal-100"
  } ${extra}`;
}

interface FieldShellProps {
  label: string;
  /** Text, or text with a link (e.g. to the student who already has an LRN). */
  error?: ReactNode;
  hint?: ReactNode;
  optional?: boolean;
  children: (ids: { id: string; describedBy?: string; invalid: boolean }) => ReactNode;
}

/** Label + control + inline error/hint, wired up for screen readers. */
export function Field({ label, error, hint, optional, children }: FieldShellProps) {
  const id = useId();
  const errId = `${id}-err`;
  const hintId = `${id}-hint`;
  const describedBy = [error ? errId : null, hint ? hintId : null].filter(Boolean).join(" ") || undefined;
  return (
    <div>
      <label htmlFor={id} className="block text-sm font-medium text-slate-700">
        {label}
        {optional && <span className="ml-1 font-normal text-slate-400">(optional)</span>}
      </label>
      {children({ id, describedBy, invalid: !!error })}
      {error ? (
        <p id={errId} className="mt-1.5 flex items-start gap-1 text-xs font-medium text-red-700">
          <AlertCircle className="mt-px size-3.5 shrink-0" aria-hidden />
          {error}
        </p>
      ) : hint ? (
        <p id={hintId} className="mt-1.5 text-xs text-slate-500">
          {hint}
        </p>
      ) : null}
    </div>
  );
}

type TextFieldProps = Omit<InputHTMLAttributes<HTMLInputElement>, "id"> & {
  label: string;
  error?: ReactNode;
  hint?: ReactNode;
  optional?: boolean;
};

export function TextField({ label, error, hint, optional, className, ...rest }: TextFieldProps) {
  return (
    <Field label={label} error={error} hint={hint} optional={optional}>
      {({ id, describedBy, invalid }) => (
        <input
          id={id}
          aria-invalid={invalid}
          aria-describedby={describedBy}
          className={inputClass(invalid, className)}
          {...rest}
        />
      )}
    </Field>
  );
}

type SelectFieldProps = Omit<SelectHTMLAttributes<HTMLSelectElement>, "id"> & {
  label: string;
  error?: string | null;
  hint?: ReactNode;
  children: ReactNode;
};

export function SelectField({ label, error, hint, className, children, ...rest }: SelectFieldProps) {
  return (
    <Field label={label} error={error} hint={hint}>
      {({ id, describedBy, invalid }) => (
        <select
          id={id}
          aria-invalid={invalid}
          aria-describedby={describedBy}
          className={inputClass(invalid, className)}
          {...rest}
        >
          {children}
        </select>
      )}
    </Field>
  );
}

const BUTTON_TONES = {
  primary: "bg-teal-700 text-white hover:bg-teal-800 disabled:bg-slate-300",
  secondary: "bg-white text-slate-700 ring-1 ring-slate-300 hover:bg-slate-50 disabled:text-slate-400",
  danger: "bg-red-600 text-white hover:bg-red-700 disabled:bg-slate-300",
  ghost: "text-slate-600 hover:bg-slate-100 disabled:text-slate-300",
};

export function buttonClass(tone: keyof typeof BUTTON_TONES = "primary", size: "sm" | "md" = "md") {
  const pad = size === "sm" ? "px-2.5 py-1.5 text-xs" : "px-4 py-2 text-sm";
  return `inline-flex items-center justify-center gap-1.5 rounded-lg font-medium transition-colors disabled:cursor-not-allowed ${pad} ${BUTTON_TONES[tone]}`;
}

export function ErrorNotice({ children }: { children: ReactNode }) {
  return (
    <div role="alert" className="flex gap-2 rounded-lg bg-red-50 p-3 text-sm text-red-900 ring-1 ring-red-200">
      <AlertCircle className="mt-0.5 size-4 shrink-0" aria-hidden />
      <div>{children}</div>
    </div>
  );
}
