import { Lock, X } from "lucide-react";
import { useEffect, useId, useRef, type ReactNode } from "react";
import type { Features } from "../lib/api";
import { usePlan } from "../lib/session";

/**
 * Wraps a Premium-only item. On Premium it renders the children as-is.
 * On Basic the item stays visible but is dimmed, can't be used, and shows a lock;
 * clicking it opens the "Upgrade to Premium" message.
 *
 * Display only: the server rejects Premium-only requests on Basic regardless.
 */
export function PremiumGate({
  feature,
  label,
  children,
}: {
  feature: keyof Features;
  /** What's locked, used in the upgrade message, e.g. "Advanced filters". */
  label: string;
  children: ReactNode;
}) {
  const { features } = usePlan();
  const dialogRef = useRef<HTMLDialogElement>(null);

  if (features[feature]) return <>{children}</>;

  return (
    <div className="relative">
      <div inert className="pointer-events-none select-none opacity-50 grayscale">
        {children}
      </div>
      <button
        type="button"
        onClick={() => dialogRef.current?.showModal()}
        className="absolute inset-0 flex cursor-pointer items-start justify-end rounded-lg p-3 outline-none ring-teal-600 focus-visible:ring-2"
        aria-label={`${label} — Premium feature, locked. Learn about upgrading.`}
      >
        <PremiumBadge />
      </button>
      <UpgradeDialog dialogRef={dialogRef} label={label} />
    </div>
  );
}

export function PremiumBadge() {
  return (
    <span className="inline-flex items-center gap-1 rounded-full bg-amber-100 px-2 py-0.5 text-xs font-semibold text-amber-800 ring-1 ring-amber-300">
      <Lock className="size-3" aria-hidden />
      Premium
    </span>
  );
}

const PREMIUM_BENEFITS = [
  "Unlimited student records (Basic: up to 500)",
  "Up to 10 user accounts (Basic: 2)",
  "Advanced search: date range, complaint and grade filters",
  "Detailed reports",
  "Daily server backups (Basic: weekly)",
];

export function UpgradeDialog({
  dialogRef,
  label,
}: {
  dialogRef: React.RefObject<HTMLDialogElement | null>;
  label: string;
}) {
  const { isAdmin } = usePlan();
  const titleId = useId();

  // Close when clicking the backdrop.
  useEffect(() => {
    const d = dialogRef.current;
    if (!d) return;
    const onClick = (e: MouseEvent) => {
      if (e.target === d) d.close();
    };
    d.addEventListener("click", onClick);
    return () => d.removeEventListener("click", onClick);
  }, [dialogRef]);

  return (
    <dialog
      ref={dialogRef}
      aria-labelledby={titleId}
      className="m-auto w-full max-w-md rounded-xl p-0 shadow-xl"
    >
      <div className="p-6">
        <div className="flex items-start justify-between gap-4">
          <div className="flex items-center gap-3">
            <span className="flex size-10 items-center justify-center rounded-full bg-amber-100 text-amber-700">
              <Lock className="size-5" aria-hidden />
            </span>
            <h2 id={titleId} className="text-lg font-semibold">
              Upgrade to Premium
            </h2>
          </div>
          <button
            type="button"
            onClick={() => dialogRef.current?.close()}
            className="rounded-md p-1 text-slate-500 hover:bg-slate-100 hover:text-slate-700"
            aria-label="Close"
          >
            <X className="size-5" aria-hidden />
          </button>
        </div>
        <p className="mt-4 text-sm text-slate-700">
          <strong className="font-semibold">{label}</strong> is part of the Premium plan.
        </p>
        <ul className="mt-3 space-y-1.5 text-sm text-slate-600">
          {PREMIUM_BENEFITS.map((b) => (
            <li key={b} className="flex gap-2">
              <span className="text-teal-600" aria-hidden>
                ✓
              </span>
              {b}
            </li>
          ))}
        </ul>
        <p className="mt-4 rounded-lg bg-slate-50 p-3 text-sm text-slate-600">
          {isAdmin
            ? "To upgrade, contact your Clinilog provider."
            : "Ask your clinic admin about upgrading to Premium."}
        </p>
        <div className="mt-5 flex justify-end">
          <button
            autoFocus
            type="button"
            onClick={() => dialogRef.current?.close()}
            className="rounded-lg bg-teal-700 px-4 py-2 text-sm font-medium text-white hover:bg-teal-800"
          >
            OK
          </button>
        </div>
      </div>
    </dialog>
  );
}
