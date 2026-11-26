import { ChevronLeft, ChevronRight, Loader2, Plus, Stethoscope } from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { PendingChanges } from "../../components/PendingChanges";
import { buttonClass, ErrorNotice, inputClass } from "../../components/form";
import { asCmdError, visitsApi, type Visit, type VisitPage } from "../../lib/api";
import { formatDate, formatDateTime, listName, sectionLabel } from "../../lib/format";
import { usePlan } from "../../lib/session";
import { todayPH } from "../../lib/validation";

const PAGE_SIZE = 25;

export function VisitsListPage() {
  const navigate = useNavigate();
  const location = useLocation();
  const notice = (location.state as { notice?: string } | null)?.notice;
  const { readOnly } = usePlan();
  const [date, setDate] = useState(todayPH());
  const [page, setPage] = useState(1);
  const [data, setData] = useState<VisitPage | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setPage(1), [date]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    visitsApi
      .list({ date: date || undefined, page, page_size: PAGE_SIZE })
      .then((d) => !cancelled && (setData(d), setError(null)))
      .catch((e) => !cancelled && setError(asCmdError(e).message))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [date, page]);

  const lastPage = data ? Math.max(1, Math.ceil(data.total / data.page_size)) : 1;
  const isToday = date === todayPH();

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <div className="flex items-end gap-2">
          <div>
            <label htmlFor="visit-date" className="block text-xs font-medium text-slate-600">
              Date
            </label>
            <input
              id="visit-date"
              type="date"
              value={date}
              max={todayPH()}
              onChange={(e) => setDate(e.target.value)}
              className={inputClass(false, "w-44")}
            />
          </div>
          {!isToday && (
            <button type="button" className={buttonClass("ghost", "sm")} onClick={() => setDate(todayPH())}>
              Today
            </button>
          )}
          <button type="button" className={buttonClass("ghost", "sm")} onClick={() => setDate("")} disabled={date === ""}>
            All dates
          </button>
        </div>
        <button
          type="button"
          className={buttonClass("primary")}
          disabled={readOnly}
          title={readOnly ? "Recording visits is paused while the subscription is inactive." : undefined}
          onClick={() => navigate("/visits/new")}
        >
          <Plus className="size-4" aria-hidden /> New visit
        </button>
      </div>

      {notice && (
        <p role="status" className="rounded-lg bg-teal-50 px-4 py-3 text-sm text-teal-900 ring-1 ring-teal-200">
          {notice}
        </p>
      )}
      <PendingChanges />
      {error && <ErrorNotice>{error}</ErrorNotice>}

      <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        <div className="border-b border-slate-200 px-4 py-3 text-sm font-semibold text-slate-800">
          {date === "" ? "All visits" : isToday ? "Today's visits" : `Visits on ${formatDate(date)}`}
          {data && <span className="ml-2 font-normal text-slate-500">({data.total})</span>}
        </div>
        <VisitTable visits={data?.items ?? []} showDate={date === ""} />

        {loading && !data && (
          <p className="flex items-center justify-center gap-2 py-12 text-sm text-slate-500">
            <Loader2 className="size-4 animate-spin" aria-hidden /> Loading visits…
          </p>
        )}
        {data && data.items.length === 0 && (
          <div className="flex flex-col items-center py-14 text-center">
            <Stethoscope className="size-8 text-slate-300" aria-hidden />
            <p className="mt-3 text-sm font-medium text-slate-700">No visits recorded{date ? " on this day" : " yet"}.</p>
          </div>
        )}
        {data && data.total > data.page_size && (
          <div className="flex items-center justify-end gap-1 border-t border-slate-200 px-4 py-3 text-sm text-slate-600">
            <button type="button" className={buttonClass("ghost", "sm")} disabled={page <= 1 || loading} onClick={() => setPage((p) => p - 1)} aria-label="Previous page">
              <ChevronLeft className="size-4" aria-hidden />
            </button>
            <span className="px-2 tabular-nums">
              Page {page} of {lastPage}
            </span>
            <button type="button" className={buttonClass("ghost", "sm")} disabled={page >= lastPage || loading} onClick={() => setPage((p) => p + 1)} aria-label="Next page">
              <ChevronRight className="size-4" aria-hidden />
            </button>
          </div>
        )}
      </div>
    </div>
  );
}

const timeFmt = new Intl.DateTimeFormat("en-PH", { timeZone: "Asia/Manila", hour: "numeric", minute: "2-digit" });

/** Visits table shared by the Visits page and the Dashboard. */
export function VisitTable({ visits, showDate }: { visits: Visit[]; showDate: boolean }) {
  const navigate = useNavigate();
  if (visits.length === 0) return null;
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-left text-sm">
        <thead className="bg-slate-50 text-xs font-semibold tracking-wide text-slate-500 uppercase">
          <tr>
            <th scope="col" className="px-4 py-2.5">{showDate ? "Date & time" : "Time"}</th>
            <th scope="col" className="px-4 py-2.5">Student</th>
            <th scope="col" className="px-4 py-2.5">Complaint</th>
            <th scope="col" className="px-4 py-2.5">Medicines</th>
            <th scope="col" className="px-4 py-2.5">Attended by</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-slate-100 align-top">
          {visits.map((v) => (
            <tr key={v.id} onClick={() => navigate(`/visits/${v.id}`)} className="cursor-pointer hover:bg-teal-50/50">
              <td className="px-4 py-3 whitespace-nowrap text-slate-600 tabular-nums">
                <Link to={`/visits/${v.id}`} onClick={(e) => e.stopPropagation()} className="outline-none hover:underline focus-visible:underline">
                  {showDate ? formatDateTime(v.visited_at) : timeFmt.format(new Date(v.visited_at))}
                </Link>
              </td>
              <td className="px-4 py-3">
                <p className="font-medium">{listName(v.student)}</p>
                <p className="text-xs text-slate-500">{sectionLabel(v.student)}</p>
              </td>
              <td className="max-w-xs px-4 py-3">
                <p className="line-clamp-2">{v.complaint}</p>
              </td>
              <td className="px-4 py-3 text-slate-600">
                {v.medicines.length === 0 ? "—" : v.medicines.map((m) => `${m.name} × ${m.quantity}`).join(", ")}
              </td>
              <td className="px-4 py-3 whitespace-nowrap text-slate-600">{v.attended_by.full_name}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
