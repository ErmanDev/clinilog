import { RefreshCw } from "lucide-react";
import { useState, type ReactNode } from "react";
import { Card } from "../components/ui";
import { formatDate, planLabel } from "../lib/format";
import { useSession } from "../lib/session";

const STATUS_LABEL: Record<string, string> = {
  active: "Active",
  expired: "Expired",
  cancelled: "Cancelled",
  none: "No subscription",
};

export function SubscriptionPage() {
  const { plan, planError, refreshPlan } = useSession();
  const [refreshing, setRefreshing] = useState(false);

  async function refresh() {
    setRefreshing(true);
    await refreshPlan();
    setRefreshing(false);
  }

  const refreshButton = (
    <button
      type="button"
      onClick={() => void refresh()}
      disabled={refreshing}
      className="inline-flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-xs font-medium text-slate-600 ring-1 ring-slate-300 hover:bg-slate-50 disabled:opacity-60"
    >
      <RefreshCw className={`size-3.5 ${refreshing ? "animate-spin" : ""}`} aria-hidden />
      Check again
    </button>
  );

  if (!plan) {
    return (
      <Card title="Subscription" action={refreshButton}>
        <p className="text-sm text-slate-600">{planError ?? "Checking…"}</p>
      </Card>
    );
  }

  const { info } = plan;
  return (
    <div className="max-w-2xl space-y-4">
      <Card title="Subscription" action={refreshButton}>
        <dl className="grid grid-cols-2 gap-x-6 gap-y-4 text-sm">
          <Item label="Plan">{planLabel(info.plan)}</Item>
          <Item label="Status">{STATUS_LABEL[info.status] ?? info.status}</Item>
          <Item label="Start date">{info.start_date ? formatDate(info.start_date) : "—"}</Item>
          <Item label="End date">{info.end_date ? formatDate(info.end_date) : "—"}</Item>
          <Item label="Student records">
            <Usage used={info.usage.students} limit={info.limits.max_students} />
          </Item>
          <Item label="Active user accounts">
            <Usage used={info.usage.active_users} limit={info.limits.max_users} />
          </Item>
          <Item label="Server backups">{info.features.daily_backups ? "Daily" : "Weekly"}</Item>
          <Item label="Last checked">
            {formatDate(plan.fetched_at)}
            {plan.source === "cache" && " (offline, last known)"}
          </Item>
        </dl>
      </Card>
      <p className="text-xs text-slate-500">To change or renew your plan, contact your Clinilog provider.</p>
    </div>
  );
}

/** "12 of 500 used" with a small meter; amber at 90% or more. `limit` null = unlimited. */
function Usage({ used, limit }: { used: number; limit: number | null }) {
  if (limit === null) return <>{used} · unlimited</>;
  const pct = Math.min(100, (used / limit) * 100);
  return (
    <div>
      <span className="tabular-nums">
        {used} of {limit} used
      </span>
      <div className="mt-1.5 h-1.5 w-40 overflow-hidden rounded-full bg-slate-200" aria-hidden>
        <div className={`h-full rounded-full ${pct >= 90 ? "bg-amber-500" : "bg-teal-600"}`} style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

function Item({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt className="text-xs font-medium text-slate-500">{label}</dt>
      <dd className="mt-0.5 font-medium text-slate-900">{children}</dd>
    </div>
  );
}
