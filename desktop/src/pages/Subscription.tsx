import { Check, CircleCheck, FlaskConical, Loader2, RefreshCw } from "lucide-react";
import { useEffect, useId, useState, type ReactNode } from "react";
import { Dialog } from "../components/Dialog";
import { buttonClass, ErrorNotice } from "../components/form";
import { Card } from "../components/ui";
import { asCmdError, subscriptionApi, type PlanName, type PlanOption, type PlansResponse, type Receipt, type Term } from "../lib/api";
import { formatDate, peso, planLabel } from "../lib/format";
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
  const [plans, setPlans] = useState<PlansResponse | null>(null);
  const [plansError, setPlansError] = useState<string | null>(null);
  const [term, setTerm] = useState<Term>("yearly");
  const [checkout, setCheckout] = useState<PlanOption | null>(null);

  useEffect(() => {
    subscriptionApi
      .plans()
      .then(setPlans)
      .catch((e) => setPlansError(asCmdError(e).message));
  }, []);

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
      <RefreshCw className={`size-3.5 ${refreshing ? "animate-spin" : ""}`} aria-hidden /> Check again
    </button>
  );

  const info = plan?.info ?? null;

  return (
    <div className="max-w-4xl space-y-6">
      {!info ? (
        <Card title="Current subscription" action={refreshButton}>
          <p className="text-sm text-slate-600">{planError ?? "Checking…"}</p>
        </Card>
      ) : (
        <Card title="Current subscription" action={refreshButton}>
          <dl className="grid grid-cols-2 gap-x-6 gap-y-4 text-sm md:grid-cols-4">
            <Item label="Plan">{planLabel(info.plan)}</Item>
            <Item label="Status">{STATUS_LABEL[info.status] ?? info.status}</Item>
            <Item label="Valid from">{info.start_date ? formatDate(info.start_date) : "—"}</Item>
            <Item label="Valid until">{info.end_date ? formatDate(info.end_date) : "—"}</Item>
            <Item label="Student records">
              <Usage used={info.usage.students} limit={info.limits.max_students} />
            </Item>
            <Item label="Active user accounts">
              <Usage used={info.usage.active_users} limit={info.limits.max_users} />
            </Item>
            <Item label="Server backups">{info.features.daily_backups ? "Daily" : "Weekly"}</Item>
            <Item label="Last checked">
              {formatDate(plan!.fetched_at)}
              {plan!.source === "cache" && " (offline)"}
            </Item>
          </dl>
        </Card>
      )}

      <section aria-labelledby="plans-title" className="space-y-3">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <div>
            <h2 id="plans-title" className="text-base font-semibold">
              Plans
            </h2>
            <p className="text-sm text-slate-500">Renew, upgrade or switch plans. This is a demo checkout — no real payment is made.</p>
          </div>
          <TermSwitch value={term} onChange={setTerm} />
        </div>

        {plansError && <ErrorNotice>{plansError}</ErrorNotice>}
        {!plans && !plansError && (
          <p className="flex items-center gap-2 text-sm text-slate-500">
            <Loader2 className="size-4 animate-spin" aria-hidden /> Loading plans…
          </p>
        )}
        {plans && (
          <div className="grid gap-4 md:grid-cols-2">
            {plans.plans.map((p) => (
              <PlanCard key={p.plan} option={p} term={term} currentPlan={info?.plan} currentActive={!!info && !info.read_only} onChoose={() => setCheckout(p)} />
            ))}
          </div>
        )}
      </section>

      {checkout && plans && info && (
        <CheckoutDialog
          option={checkout}
          term={term}
          methods={plans.payment_methods}
          current={{ plan: info.plan, active: !info.read_only, end: info.end_date }}
          onClose={() => setCheckout(null)}
          onPaid={() => void refreshPlan()}
        />
      )}
    </div>
  );
}

function TermSwitch({ value, onChange }: { value: Term; onChange: (t: Term) => void }) {
  return (
    <div role="radiogroup" aria-label="Billing period" className="inline-flex rounded-lg bg-slate-200/70 p-1 text-sm">
      {(["monthly", "yearly"] as const).map((t) => (
        <button
          key={t}
          type="button"
          role="radio"
          aria-checked={value === t}
          onClick={() => onChange(t)}
          className={`rounded-md px-3 py-1 font-medium ${value === t ? "bg-white text-slate-900 shadow-sm" : "text-slate-600 hover:text-slate-900"}`}
        >
          {t === "monthly" ? "Monthly" : "Yearly"}
        </button>
      ))}
    </div>
  );
}

const FEATURES = (o: PlanOption) => [
  o.limits.max_students === null ? "Unlimited student records" : `Up to ${o.limits.max_students} student records`,
  `Up to ${o.limits.max_users} user accounts`,
  o.features.advanced_search ? "Advanced search (date range, complaint, grade)" : "Search by name and section",
  o.features.detailed_reports ? "Detailed reports" : "Basic reports",
  o.features.daily_backups ? "Daily server backups" : "Weekly server backups",
];

function PlanCard({
  option,
  term,
  currentPlan,
  currentActive,
  onChoose,
}: {
  option: PlanOption;
  term: Term;
  currentPlan?: PlanName;
  currentActive: boolean;
  onChoose: () => void;
}) {
  const isCurrent = currentPlan === option.plan;
  const premium = option.plan === "premium";
  const price = term === "monthly" ? option.monthly_php : option.yearly_php;
  const action = isCurrent ? `Renew ${planLabel(option.plan)}` : premium ? "Upgrade to Premium" : "Switch to Basic";

  return (
    <div className={`flex flex-col rounded-xl border bg-white p-5 shadow-sm ${premium ? "border-amber-300 ring-1 ring-amber-200" : "border-slate-200"}`}>
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-lg font-semibold">{planLabel(option.plan)}</h3>
        {isCurrent && (
          <span className="inline-flex items-center gap-1 rounded-full bg-teal-50 px-2 py-0.5 text-xs font-semibold text-teal-800 ring-1 ring-teal-200">
            <CircleCheck className="size-3" aria-hidden /> {currentActive ? "Current plan" : "Your plan (ended)"}
          </span>
        )}
      </div>
      <p className="mt-2">
        <span className="text-3xl font-semibold tabular-nums">{peso(price)}</span>
        <span className="text-sm text-slate-500"> / {term === "monthly" ? "month" : "year"}</span>
      </p>
      {term === "yearly" && (
        <p className="text-xs text-slate-500">
          {peso(Math.round(option.yearly_php / 12))} a month, billed yearly · sample price
        </p>
      )}
      {term === "monthly" && <p className="text-xs text-slate-500">Sample price</p>}
      <ul className="mt-4 flex-1 space-y-2 text-sm text-slate-700">
        {FEATURES(option).map((f) => (
          <li key={f} className="flex gap-2">
            <Check className="mt-0.5 size-4 shrink-0 text-teal-700" aria-hidden />
            {f}
          </li>
        ))}
      </ul>
      <button type="button" className={`${buttonClass(isCurrent ? "secondary" : "primary")} mt-5`} onClick={onChoose}>
        {action}
      </button>
    </div>
  );
}

/** When the new plan would start, mirroring the server's rule (the receipt shows the real dates). */
function startText(option: PlanOption, current: { plan: PlanName; active: boolean; end: string | null }) {
  if (current.active && current.plan === option.plan && current.end) {
    return `Adds to your current plan — starts after ${formatDate(current.end)}`;
  }
  if (current.active) return `Starts today and replaces your ${planLabel(current.plan)} plan`;
  return "Starts today";
}

function CheckoutDialog({
  option,
  term,
  methods,
  current,
  onClose,
  onPaid,
}: {
  option: PlanOption;
  term: Term;
  methods: { id: string; label: string }[];
  current: { plan: PlanName; active: boolean; end: string | null };
  onClose: () => void;
  onPaid: () => void;
}) {
  const [method, setMethod] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [receipt, setReceipt] = useState<Receipt | null>(null);
  const groupId = useId();
  const price = term === "monthly" ? option.monthly_php : option.yearly_php;

  async function pay() {
    setBusy(true);
    setError(null);
    try {
      const r = await subscriptionApi.checkout(option.plan, term, method);
      setReceipt(r);
      onPaid();
    } catch (e) {
      setError(asCmdError(e).message);
    } finally {
      setBusy(false);
    }
  }

  if (receipt) {
    return (
      <Dialog
        open
        onClose={onClose}
        title="Payment confirmed (demo)"
        footer={
          <button type="button" className={buttonClass("primary")} onClick={onClose} autoFocus>
            Done
          </button>
        }
      >
        <div className="space-y-3">
          <p className="flex items-center gap-2 font-medium text-teal-800">
            <CircleCheck className="size-5" aria-hidden /> Your school is now on {planLabel(receipt.plan)}.
          </p>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-2 rounded-lg bg-slate-50 p-3 text-sm">
            <dt className="text-slate-500">Reference</dt>
            <dd className="font-mono">{receipt.reference}</dd>
            <dt className="text-slate-500">Plan</dt>
            <dd>
              {planLabel(receipt.plan)}, {receipt.term}
            </dd>
            <dt className="text-slate-500">Period</dt>
            <dd>
              {formatDate(receipt.start_date)} – {formatDate(receipt.end_date)}
            </dd>
            <dt className="text-slate-500">Amount</dt>
            <dd>
              {peso(receipt.amount_php)} <span className="text-xs text-slate-500">(not charged)</span>
            </dd>
            <dt className="text-slate-500">Method</dt>
            <dd>{receipt.payment_method}</dd>
          </dl>
          <DemoNote />
        </div>
      </Dialog>
    );
  }

  return (
    <Dialog
      open
      onClose={() => !busy && onClose()}
      title={`${current.plan === option.plan ? "Renew" : "Get"} ${planLabel(option.plan)} — ${term}`}
      footer={
        <>
          <button type="button" className={buttonClass("secondary")} onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className={buttonClass("primary")} onClick={() => void pay()} disabled={!method || busy}>
            {busy && <Loader2 className="size-4 animate-spin" aria-hidden />}
            Confirm demo payment · {peso(price)}
          </button>
        </>
      }
    >
      <div className="space-y-4">
        <DemoNote />
        <dl className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-sm">
          <dt className="text-slate-500">Plan</dt>
          <dd className="font-medium">
            {planLabel(option.plan)} ({term})
          </dd>
          <dt className="text-slate-500">Amount</dt>
          <dd className="font-medium">{peso(price)}</dd>
          <dt className="text-slate-500">When</dt>
          <dd>{startText(option, current)}</dd>
        </dl>
        <fieldset>
          <legend id={groupId} className="text-sm font-medium text-slate-700">
            Payment method
          </legend>
          <div role="radiogroup" aria-labelledby={groupId} className="mt-2 grid grid-cols-2 gap-2">
            {methods.map((m) => (
              <label
                key={m.id}
                className={`flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-2 text-sm ${
                  method === m.id ? "border-teal-600 bg-teal-50 ring-1 ring-teal-600" : "border-slate-300 hover:bg-slate-50"
                }`}
              >
                <input type="radio" name="method" value={m.id} checked={method === m.id} onChange={() => setMethod(m.id)} className="accent-teal-700" />
                {m.label}
              </label>
            ))}
          </div>
        </fieldset>
        {error && <ErrorNotice>{error}</ErrorNotice>}
      </div>
    </Dialog>
  );
}

function DemoNote() {
  return (
    <p className="flex items-start gap-2 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-900 ring-1 ring-amber-200">
      <FlaskConical className="mt-0.5 size-3.5 shrink-0" aria-hidden />
      <span>
        <strong className="font-semibold">Demo checkout.</strong> No money is charged and no payment details are collected. Prices are
        samples.
      </span>
    </p>
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
      <div className="mt-1.5 h-1.5 w-32 overflow-hidden rounded-full bg-slate-200" aria-hidden>
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

