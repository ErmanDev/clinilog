import { ChevronRight, Loader2, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { useNavigate, useSearchParams } from "react-router-dom";
import { ErrorNotice } from "../../components/form";
import { planTermLabel, RequestStatusBadge } from "../../components/payments";
import { asCmdError, superadminApi, type RequestStatus, type ReviewItem } from "../../lib/api";
import { formatDateTime, peso } from "../../lib/format";

const TABS: { id: RequestStatus | "all"; label: string }[] = [
  { id: "pending", label: "Waiting for review" },
  { id: "approved", label: "Approved" },
  { id: "rejected", label: "Not approved" },
  { id: "cancelled", label: "Cancelled" },
  { id: "all", label: "All" },
];

const POLL_MS = 60_000;

/** Superadmin home: payments sent by schools, oldest waiting first. */
export function PaymentRequestsPage() {
  const [params, setParams] = useSearchParams();
  const tab = (TABS.find((t) => t.id === params.get("status"))?.id ?? "pending") as RequestStatus | "all";
  const [items, setItems] = useState<ReviewItem[] | null>(null);
  const [pendingCount, setPendingCount] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const navigate = useNavigate();

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const [list, pending] = await Promise.all([
        superadminApi.requests(tab),
        tab === "pending" ? null : superadminApi.requests("pending"),
      ]);
      setItems(list);
      setPendingCount((pending ?? list).length);
      setError(null);
    } catch (e) {
      setError(asCmdError(e).message);
    } finally {
      setLoading(false);
    }
  }, [tab]);

  useEffect(() => {
    setItems(null);
    void load();
    const id = window.setInterval(() => void load(), POLL_MS);
    return () => window.clearInterval(id);
  }, [load]);

  return (
    <div className="max-w-6xl space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div role="tablist" aria-label="Status" className="inline-flex flex-wrap rounded-lg bg-slate-200/70 p-1 text-sm">
          {TABS.map((t) => (
            <button
              key={t.id}
              type="button"
              role="tab"
              aria-selected={tab === t.id}
              onClick={() => setParams(t.id === "pending" ? {} : { status: t.id })}
              className={`rounded-md px-3 py-1 font-medium ${tab === t.id ? "bg-white text-slate-900 shadow-sm" : "text-slate-600 hover:text-slate-900"}`}
            >
              {t.label}
              {t.id === "pending" && pendingCount !== null && pendingCount > 0 && (
                <span className="ml-1.5 rounded-full bg-amber-500 px-1.5 text-xs font-semibold text-white">{pendingCount}</span>
              )}
            </button>
          ))}
        </div>
        <button
          type="button"
          onClick={() => void load()}
          disabled={loading}
          className="inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs font-medium text-slate-600 ring-1 ring-slate-300 hover:bg-slate-50 disabled:opacity-60"
        >
          <RefreshCw className={`size-3.5 ${loading ? "animate-spin" : ""}`} aria-hidden /> Refresh
        </button>
      </div>

      {tab === "pending" && (
        <p className="text-sm text-slate-500">
          Open a request, check your GCash for a payment with the same reference number and amount, then approve or reject it.
        </p>
      )}
      {error && <ErrorNotice>{error}</ErrorNotice>}

      <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        {!items && !error && (
          <p className="flex items-center justify-center gap-2 py-12 text-sm text-slate-500">
            <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
          </p>
        )}
        {items && items.length === 0 && (
          <p className="py-12 text-center text-sm text-slate-500">
            {tab === "pending" ? "No payments waiting for review." : "Nothing here yet."}
          </p>
        )}
        {items && items.length > 0 && (
          <table className="w-full text-left text-sm">
            <thead className="border-b border-slate-200 bg-slate-50 text-xs font-semibold tracking-wide text-slate-500 uppercase">
              <tr>
                <th scope="col" className="px-4 py-3">Sent</th>
                <th scope="col" className="px-4 py-3">School</th>
                <th scope="col" className="px-4 py-3">Plan</th>
                <th scope="col" className="px-4 py-3 text-right">Amount</th>
                <th scope="col" className="px-4 py-3">Reference</th>
                <th scope="col" className="px-4 py-3">Status</th>
                <th scope="col" className="px-4 py-3">
                  <span className="sr-only">Open</span>
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {items.map((r) => (
                <tr key={r.id} className="cursor-pointer hover:bg-slate-50" onClick={() => navigate(`/requests/${r.id}`)}>
                  <td className="px-4 py-3 whitespace-nowrap text-slate-600">{formatDateTime(r.created_at)}</td>
                  <td className="px-4 py-3">
                    <span className="font-medium">{r.school_name}</span>
                    <span className="block text-xs text-slate-500">
                      {r.requested_by_name} ({r.requested_by_username})
                    </span>
                  </td>
                  <td className="px-4 py-3">{planTermLabel(r.plan, r.term)}</td>
                  <td className="px-4 py-3 text-right font-medium tabular-nums">{peso(r.amount_php)}</td>
                  <td className="px-4 py-3 font-mono text-slate-700">{r.reference_no}</td>
                  <td className="px-4 py-3">
                    <RequestStatusBadge status={r.status} />
                  </td>
                  <td className="px-4 py-3 text-right">
                    <a
                      href={`#/requests/${r.id}`}
                      onClick={(e) => e.stopPropagation()}
                      className="inline-flex items-center gap-0.5 text-sm font-medium text-teal-700 hover:underline"
                    >
                      {r.status === "pending" ? "Review" : "View"} <ChevronRight className="size-4" aria-hidden />
                    </a>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
