import { Loader2, Plus, Stethoscope } from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { buttonClass, ErrorNotice } from "../components/form";
import { PendingVisits } from "../components/PendingVisits";
import { Card } from "../components/ui";
import { asCmdError, visitsApi, type Dashboard } from "../lib/api";
import { formatDate, planLabel } from "../lib/format";
import { usePlan, useSession } from "../lib/session";
import { VisitTable } from "./visits/VisitsList";

const REFRESH_MS = 60_000;

export function DashboardPage() {
  const { session, plan } = useSession();
  const { readOnly } = usePlan();
  const [data, setData] = useState<Dashboard | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const load = () =>
      visitsApi
        .dashboard()
        .then((d) => !cancelled && (setData(d), setError(null)))
        .catch((e) => !cancelled && setError(asCmdError(e).message));
    void load();
    const t = window.setInterval(load, REFRESH_MS);
    return () => {
      cancelled = true;
      window.clearInterval(t);
    };
  }, []);

  if (!session) return null;
  const firstName = session.user.full_name.split(" ")[0];

  return (
    <div className="space-y-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-slate-600">
          Good day, <span className="font-medium text-slate-900">{firstName}</span>.
          {data && <span className="text-slate-500"> Today is {formatDate(data.today)}.</span>}
        </p>
        {!readOnly && (
          <Link to="/visits/new" className={buttonClass("primary")}>
            <Plus className="size-4" aria-hidden /> New visit
          </Link>
        )}
      </div>

      <PendingVisits />
      {error && <ErrorNotice>{error}</ErrorNotice>}

      <div className="grid gap-4 md:grid-cols-3">
        <Card title="Visits today">
          {data ? (
            <Link to="/visits" className="text-4xl font-semibold tabular-nums text-teal-800 hover:underline">
              {data.today_count}
            </Link>
          ) : (
            <Loader2 className="size-6 animate-spin text-slate-300" aria-label="Loading" />
          )}
        </Card>
        <Card title="Plan">
          {plan ? (
            <>
              <p className="text-2xl font-semibold">{planLabel(plan.info.plan)}</p>
              {plan.info.end_date && (
                <p className="mt-1 text-xs text-slate-500">
                  {plan.info.read_only ? "Ended" : "Valid until"} {formatDate(plan.info.end_date)}
                </p>
              )}
            </>
          ) : (
            <p className="text-sm text-slate-500">Checking…</p>
          )}
        </Card>
        <Card title="Signed in as">
          <p className="truncate text-lg font-semibold">{session.user.full_name}</p>
          <p className="mt-1 text-xs capitalize text-slate-500">{session.user.role}</p>
        </Card>
      </div>

      <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        <div className="flex items-center justify-between border-b border-slate-200 px-4 py-3">
          <h2 className="text-sm font-semibold text-slate-800">Recent visits</h2>
          <Link to="/visits" className="text-xs font-medium text-teal-800 hover:underline">
            View all
          </Link>
        </div>
        {data && data.recent.length === 0 ? (
          <div className="flex flex-col items-center py-12 text-center">
            <Stethoscope className="size-8 text-slate-300" aria-hidden />
            <p className="mt-3 text-sm text-slate-600">No visits recorded yet.</p>
          </div>
        ) : (
          <VisitTable visits={data?.recent ?? []} showDate />
        )}
      </div>
    </div>
  );
}
