import { AlertTriangle, Clock, RotateCcw, Trash2 } from "lucide-react";
import { useEffect, useState } from "react";
import { asCmdError, syncApi, type OutboxEntry } from "../lib/api";
import { formatDateTime } from "../lib/format";
import { useSync } from "../lib/sync";
import { ConfirmDialog } from "./Dialog";
import { buttonClass } from "./form";

/** Changes saved on this PC that haven't reached the server yet. Hidden when there are none. */
export function PendingChanges() {
  const { status, syncNow, syncing } = useSync();
  const [items, setItems] = useState<OutboxEntry[]>([]);
  const [toDiscard, setToDiscard] = useState<OutboxEntry | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reload whenever the sync status changes (upload finished, new change saved, …).
  useEffect(() => {
    syncApi.outbox().then(setItems).catch(() => setItems([]));
  }, [status]);

  if (items.length === 0) return null;
  const mine = items.filter((i) => i.mine);
  const others = items.length - mine.length;

  async function retry(id: string) {
    await syncApi.retry(id).catch(() => {});
    await syncNow();
  }

  async function discard() {
    if (!toDiscard) return;
    setBusy(true);
    setError(null);
    try {
      await syncApi.discard(toDiscard.id);
      setItems((x) => x.filter((i) => i.id !== toDiscard.id));
      setToDiscard(null);
    } catch (e) {
      setError(asCmdError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="overflow-hidden rounded-xl border border-amber-200 bg-amber-50/40 shadow-sm">
      <div className="flex items-center justify-between gap-3 border-b border-amber-200 px-4 py-3">
        <h2 className="text-sm font-semibold text-slate-800">
          Waiting to upload <span className="font-normal text-slate-500">({items.length})</span>
        </h2>
        <button type="button" className={buttonClass("secondary", "sm")} onClick={() => void syncNow()} disabled={syncing}>
          <RotateCcw className={`size-3.5 ${syncing ? "animate-spin" : ""}`} aria-hidden /> Upload now
        </button>
      </div>
      <ul className="divide-y divide-amber-100">
        {mine.map((i) => (
          <li key={i.id} className="flex items-start gap-3 px-4 py-3 text-sm">
            {i.status === "failed" ? (
              <AlertTriangle className="mt-0.5 size-4 shrink-0 text-red-600" aria-label="Needs attention" />
            ) : (
              <Clock className="mt-0.5 size-4 shrink-0 text-amber-700" aria-label="Waiting" />
            )}
            <div className="min-w-0 flex-1">
              <p>
                <span className="text-slate-600">{i.action}:</span> <span className="font-medium">{i.label}</span>
                <span className="text-slate-500"> · {formatDateTime(i.at)}</span>
              </p>
              {i.summary && <p className="truncate text-slate-600">{i.summary}</p>}
              {i.status === "failed" && i.last_error && (
                <p className="mt-1 text-xs font-medium text-red-700">
                  The server didn't accept this change: {i.last_error} Fix the cause and try again, or discard it.
                </p>
              )}
            </div>
            {i.status === "failed" && (
              <div className="flex shrink-0 gap-1">
                <button type="button" className={buttonClass("ghost", "sm")} onClick={() => void retry(i.id)}>
                  <RotateCcw className="size-3.5" aria-hidden /> Try again
                </button>
                <button type="button" className={buttonClass("ghost", "sm")} onClick={() => setToDiscard(i)}>
                  <Trash2 className="size-3.5 text-red-600" aria-hidden /> Discard
                </button>
              </div>
            )}
          </li>
        ))}
      </ul>
      {others > 0 && (
        <p className="border-t border-amber-100 px-4 py-2.5 text-xs text-slate-600">
          {others} more saved by other staff on this PC. They upload when that person logs in here.
        </p>
      )}
      <ConfirmDialog
        open={toDiscard !== null}
        title="Discard this change?"
        confirmLabel="Discard change"
        busy={busy}
        error={error}
        onCancel={() => setToDiscard(null)}
        onConfirm={() => void discard()}
      >
        <p>
          <strong className="font-semibold">
            {toDiscard?.action}: {toDiscard?.label}
          </strong>{" "}
          was never uploaded. Discarding deletes it from this PC for good.
        </p>
      </ConfirmDialog>
    </section>
  );
}
