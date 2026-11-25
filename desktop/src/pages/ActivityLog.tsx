import { Eye, Loader2, Pencil, Plus, Trash2, X, type LucideIcon } from "lucide-react";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { buttonClass, ErrorNotice, inputClass } from "../components/form";
import { Pagination } from "../components/Pagination";
import { activityApi, asCmdError, usersApi, type Account, type ActivityEntry, type ActivityPage } from "../lib/api";
import { formatDateTime } from "../lib/format";
import { todayPH } from "../lib/validation";

const PAGE_SIZES = [25, 50, 100];
const PAGE_SIZE_KEY = "clinilog.activity.pageSize";

/** The rows-per-page choice is remembered on this PC (falls back to 50). */
function savedPageSize(): number {
  try {
    const n = Number(localStorage.getItem(PAGE_SIZE_KEY));
    return PAGE_SIZES.includes(n) ? n : 50;
  } catch {
    return 50;
  }
}

const ACTIONS: Record<ActivityEntry["action"], { label: string; icon: LucideIcon }> = {
  view: { label: "Viewed", icon: Eye },
  create: { label: "Created", icon: Plus },
  update: { label: "Changed", icon: Pencil },
  delete: { label: "Deleted", icon: Trash2 },
};

const TABLES: Record<ActivityEntry["table_name"], string> = {
  students: "Student",
  clinic_visits: "Clinic visit",
  users: "User account",
  subscriptions: "Subscription",
  subscription_requests: "Payment",
};

export function ActivityLogPage() {
  const [users, setUsers] = useState<Account[]>([]);
  const [userId, setUserId] = useState("");
  const [action, setAction] = useState("");
  const [table, setTable] = useState("");
  const [from, setFrom] = useState("");
  const [to, setTo] = useState("");
  const [page, setPage] = useState(1);
  const [pageSize, setPageSize] = useState(savedPageSize);
  const topRef = useRef<HTMLDivElement>(null);
  const [data, setData] = useState<ActivityPage | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    usersApi.list().then(setUsers).catch(() => {});
  }, []);

  useEffect(() => setPage(1), [userId, action, table, from, to, pageSize]);

  function changePageSize(size: number) {
    setPageSize(size);
    try {
      localStorage.setItem(PAGE_SIZE_KEY, String(size));
    } catch {
      /* not saved; fine */
    }
  }

  function goToPage(p: number) {
    setPage(p);
    topRef.current?.scrollIntoView({ block: "start" });
  }

  const rangeError = from && to && from > to ? "The 'from' date must be on or before the 'to' date." : null;

  useEffect(() => {
    if (rangeError) return;
    let cancelled = false;
    setLoading(true);
    activityApi
      .list({
        user_id: userId || undefined,
        action: action || undefined,
        table: table || undefined,
        date_from: from || undefined,
        date_to: to || undefined,
        page,
        page_size: pageSize,
      })
      .then((d) => !cancelled && (setData(d), setError(null)))
      .catch((e) => !cancelled && setError(asCmdError(e).message))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [userId, action, table, from, to, page, pageSize, rangeError]);

  const filtered = !!(userId || action || table || from || to);

  return (
    <div ref={topRef} className="scroll-mt-6 space-y-4">
      <p className="text-sm text-slate-600">
        Every time a student or visit record is viewed, added, changed or deleted, and every change to a user account.
      </p>

      <div className="flex flex-wrap items-end gap-3">
        <Filter label="User" htmlFor="a-user">
          <select id="a-user" value={userId} onChange={(e) => setUserId(e.target.value)} className={inputClass(false, "w-52")}>
            <option value="">Everyone</option>
            {users.map((u) => (
              <option key={u.id} value={u.id}>
                {u.full_name}
                {u.is_active ? "" : " (disabled)"}
              </option>
            ))}
          </select>
        </Filter>
        <Filter label="Action" htmlFor="a-action">
          <select id="a-action" value={action} onChange={(e) => setAction(e.target.value)} className={inputClass(false, "w-36")}>
            <option value="">Any action</option>
            {Object.entries(ACTIONS).map(([k, v]) => (
              <option key={k} value={k}>
                {v.label}
              </option>
            ))}
          </select>
        </Filter>
        <Filter label="Record" htmlFor="a-table">
          <select id="a-table" value={table} onChange={(e) => setTable(e.target.value)} className={inputClass(false, "w-40")}>
            <option value="">Any record</option>
            {Object.entries(TABLES).map(([k, v]) => (
              <option key={k} value={k}>
                {v}
              </option>
            ))}
          </select>
        </Filter>
        <Filter label="From" htmlFor="a-from">
          <input id="a-from" type="date" value={from} max={to || todayPH()} onChange={(e) => setFrom(e.target.value)} className={inputClass(!!rangeError, "w-40")} />
        </Filter>
        <Filter label="To" htmlFor="a-to">
          <input id="a-to" type="date" value={to} min={from || undefined} max={todayPH()} onChange={(e) => setTo(e.target.value)} className={inputClass(!!rangeError, "w-40")} />
        </Filter>
        {filtered && (
          <button
            type="button"
            className={`${buttonClass("ghost", "sm")} mb-1`}
            onClick={() => {
              setUserId("");
              setAction("");
              setTable("");
              setFrom("");
              setTo("");
            }}
          >
            <X className="size-3.5" aria-hidden /> Clear
          </button>
        )}
      </div>
      {rangeError && <p className="text-xs font-medium text-red-700">{rangeError}</p>}
      {error && <ErrorNotice>{error}</ErrorNotice>}

      <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        {loading && !data && (
          <p className="flex items-center justify-center gap-2 py-12 text-sm text-slate-500">
            <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
          </p>
        )}
        {data && data.items.length === 0 && (
          <p className="py-12 text-center text-sm text-slate-500">{filtered ? "No activity matches these filters." : "No activity yet."}</p>
        )}
        {data && data.items.length > 0 && (
          <table className={`w-full text-left text-sm transition-opacity ${loading ? "opacity-50" : ""}`} aria-busy={loading}>
            <thead className="border-b border-slate-200 bg-slate-50 text-xs font-semibold tracking-wide text-slate-500 uppercase">
              <tr>
                <th scope="col" className="px-4 py-3">When</th>
                <th scope="col" className="px-4 py-3">User</th>
                <th scope="col" className="px-4 py-3">Action</th>
                <th scope="col" className="px-4 py-3">Record</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {data.items.map((e) => {
                const a = ACTIONS[e.action];
                return (
                  <tr key={e.id}>
                    <td className="px-4 py-2.5 whitespace-nowrap text-slate-600 tabular-nums">{formatDateTime(e.created_at)}</td>
                    <td className="px-4 py-2.5">
                      {e.user_full_name} <span className="text-xs text-slate-400">{e.user_username}</span>
                    </td>
                    <td className="px-4 py-2.5">
                      <span className="inline-flex items-center gap-1.5 rounded-md bg-slate-100 px-2 py-0.5 text-xs font-medium text-slate-700">
                        <a.icon className="size-3.5" aria-hidden />
                        {a.label}
                      </span>
                    </td>
                    <td className="px-4 py-2.5">
                      <span className="mr-1.5 text-xs text-slate-500">{TABLES[e.table_name]}:</span>
                      <RecordLink entry={e} />
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
        {data && data.total > 0 && (
          <Pagination
            page={page}
            pageSize={pageSize}
            total={data.total}
            onPage={goToPage}
            onPageSize={changePageSize}
            pageSizes={PAGE_SIZES}
            busy={loading}
          />
        )}
      </div>
    </div>
  );
}

function RecordLink({ entry }: { entry: ActivityEntry }) {
  if (entry.record_label === null) return <span className="text-slate-400 italic">Deleted record</span>;
  const to =
    entry.table_name === "students"
      ? `/students/${entry.record_id}`
      : entry.table_name === "clinic_visits"
        ? `/visits/${entry.record_id}`
        : entry.table_name === "users"
          ? "/users"
          : "/subscription";
  return (
    <Link to={to} className="font-medium text-teal-800 hover:underline">
      {entry.record_label}
    </Link>
  );
}

function Filter({ label, htmlFor, children }: { label: string; htmlFor: string; children: ReactNode }) {
  return (
    <div>
      <label htmlFor={htmlFor} className="block text-xs font-medium text-slate-600">
        {label}
      </label>
      {children}
    </div>
  );
}
