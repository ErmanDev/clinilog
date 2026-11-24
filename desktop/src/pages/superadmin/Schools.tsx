import { Loader2 } from "lucide-react";
import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { ErrorNotice } from "../../components/form";
import { asCmdError, superadminApi, type SchoolSummary } from "../../lib/api";
import { formatDate, formatDateTime, planLabel } from "../../lib/format";

const STATUS: Record<string, { label: string; className: string }> = {
  active: { label: "Active", className: "text-teal-800" },
  expired: { label: "Expired", className: "text-red-700" },
  cancelled: { label: "Cancelled", className: "text-slate-500" },
};

/** Every school using Clinilog and where its subscription stands. */
export function SchoolsPage() {
  const [schools, setSchools] = useState<SchoolSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    superadminApi
      .schools()
      .then(setSchools)
      .catch((e) => setError(asCmdError(e).message));
  }, []);

  return (
    <div className="max-w-5xl space-y-4">
      {error && <ErrorNotice>{error}</ErrorNotice>}
      <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        {!schools && !error && (
          <p className="flex items-center justify-center gap-2 py-12 text-sm text-slate-500">
            <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
          </p>
        )}
        {schools && (
          <table className="w-full text-left text-sm">
            <thead className="border-b border-slate-200 bg-slate-50 text-xs font-semibold tracking-wide text-slate-500 uppercase">
              <tr>
                <th scope="col" className="px-4 py-3">School</th>
                <th scope="col" className="px-4 py-3">Plan</th>
                <th scope="col" className="px-4 py-3">Status</th>
                <th scope="col" className="px-4 py-3">Valid until</th>
                <th scope="col" className="px-4 py-3">Last approved payment</th>
                <th scope="col" className="px-4 py-3">Waiting for review</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {schools.map((s) => {
                const st = s.status ? STATUS[s.status] : null;
                return (
                  <tr key={s.id}>
                    <td className="px-4 py-3 font-medium">{s.name}</td>
                    <td className="px-4 py-3">{s.plan ? planLabel(s.plan) : "—"}</td>
                    <td className={`px-4 py-3 font-medium ${st?.className ?? "text-slate-500"}`}>{st?.label ?? "No subscription"}</td>
                    <td className="px-4 py-3 text-slate-600">{s.end_date ? formatDate(s.end_date) : "—"}</td>
                    <td className="px-4 py-3 text-slate-600">{s.last_paid_at ? formatDateTime(s.last_paid_at) : "—"}</td>
                    <td className="px-4 py-3">
                      {s.pending_requests > 0 ? (
                        <Link to="/" className="font-medium text-amber-800 hover:underline">
                          {s.pending_requests} payment{s.pending_requests === 1 ? "" : "s"}
                        </Link>
                      ) : (
                        <span className="text-slate-400">None</span>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
