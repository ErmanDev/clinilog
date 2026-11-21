import { CloudOff, KeyRound, Loader2, RefreshCw, Wifi } from "lucide-react";
import { useSync } from "../lib/sync";

/** Sidebar card: online/offline, visits waiting to upload, and "Sync now". */
export function SyncIndicator() {
  const { status, syncing, syncNow } = useSync();
  if (!status) return null;
  const waiting = status.pending + status.failed;
  const online = status.online && !status.session_offline;

  return (
    <div className="mx-3 mb-2 rounded-lg border border-slate-200 bg-slate-50 px-3 py-2.5 text-xs" role="status" aria-live="polite">
      <div className="flex items-center justify-between gap-2">
        <span className="inline-flex items-center gap-1.5 font-medium text-slate-700">
          {online ? <Wifi className="size-3.5 text-teal-700" aria-hidden /> : <CloudOff className="size-3.5 text-slate-500" aria-hidden />}
          {online ? "Online" : "Offline"}
        </span>
        <button
          type="button"
          onClick={() => void syncNow()}
          disabled={syncing}
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 font-medium text-teal-800 hover:bg-teal-50 disabled:text-slate-400"
          title="Check the connection and upload now"
        >
          {syncing ? <Loader2 className="size-3 animate-spin" aria-hidden /> : <RefreshCw className="size-3" aria-hidden />}
          Sync now
        </button>
      </div>
      <p className="mt-1 text-slate-600">
        {waiting === 0 ? "All visits uploaded." : `${waiting} visit${waiting === 1 ? "" : "s"} waiting to upload`}
        {status.failed > 0 && <span className="font-medium text-red-700"> · {status.failed} need attention</span>}
      </p>
      {status.others_waiting > 0 && (
        <p className="mt-0.5 text-slate-500">
          +{status.others_waiting} from other staff (they upload when that person logs in)
        </p>
      )}
      {status.auth_required && (
        <p className="mt-1 inline-flex items-center gap-1 font-medium text-amber-800">
          <KeyRound className="size-3" aria-hidden /> Log in again to upload
        </p>
      )}
    </div>
  );
}

/** Top-of-page notice while the app is working without the server. */
export function OfflineBanner() {
  const { status } = useSync();
  if (!status) return null;
  if (status.cache_error) {
    return <Banner tone="danger">{status.cache_error} Visits can't be saved while offline on this PC.</Banner>;
  }
  if (status.auth_required) {
    return (
      <Banner tone="warn">
        Your password or account changed since you logged in, so visits can't be uploaded. Log out and log in again — visits saved on
        this PC are kept.
      </Banner>
    );
  }
  if (!status.online || status.session_offline) {
    return (
      <Banner tone="warn">
        You're offline. New clinic visits are saved on this PC and upload automatically when the connection is back. Other pages need
        the server.
      </Banner>
    );
  }
  return null;
}

function Banner({ tone, children }: { tone: "warn" | "danger"; children: React.ReactNode }) {
  const cls = tone === "danger" ? "bg-red-50 text-red-900 ring-red-200" : "bg-amber-50 text-amber-900 ring-amber-200";
  return (
    <div role="status" className={`mx-8 mt-4 flex items-start gap-2 rounded-lg px-4 py-3 text-sm ring-1 ${cls}`}>
      <CloudOff className="mt-0.5 size-4 shrink-0" aria-hidden />
      <p>{children}</p>
    </div>
  );
}
