import { Check, CircleCheck, Clock, ImageUp, Loader2, RefreshCw, XCircle } from "lucide-react";
import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { ConfirmDialog, Dialog } from "../components/Dialog";
import { buttonClass, ErrorNotice, Field, inputClass, TextField } from "../components/form";
import { planTermLabel, readReceiptImage, RequestStatusBadge } from "../components/payments";
import { Card } from "../components/ui";
import {
  asCmdError,
  subscriptionApi,
  type PaymentInfo,
  type PaymentRequest,
  type PlanName,
  type PlanOption,
  type PlansResponse,
  type Term,
} from "../lib/api";
import { formatDate, formatDateTime, peso, planLabel } from "../lib/format";
import { useSession } from "../lib/session";

const STATUS_LABEL: Record<string, string> = {
  active: "Active",
  expired: "Expired",
  cancelled: "Cancelled",
  none: "No subscription",
};

/** While a payment is waiting for review, check for the superadmin's decision this often. */
const PENDING_POLL_MS = 60_000;
/** Show the "approved" / "not approved" notice for this long after the review. */
const RECENT_MS = 14 * 24 * 60 * 60 * 1000;

export function SubscriptionPage() {
  const { plan, planError, refreshPlan } = useSession();
  const [refreshing, setRefreshing] = useState(false);
  const [plans, setPlans] = useState<PlansResponse | null>(null);
  const [plansError, setPlansError] = useState<string | null>(null);
  const [requests, setRequests] = useState<PaymentRequest[] | null>(null);
  const [requestsError, setRequestsError] = useState<string | null>(null);
  const [term, setTerm] = useState<Term>("yearly");
  const [paying, setPaying] = useState<PlanOption | null>(null);
  const [cancelling, setCancelling] = useState<PaymentRequest | null>(null);
  const lastPending = useRef<string | null>(null);

  const loadRequests = useCallback(async () => {
    try {
      const list = await subscriptionApi.requests();
      setRequests(list);
      setRequestsError(null);
      // A payment we were waiting on was just reviewed: the plan may have changed.
      const pendingId = list.find((r) => r.status === "pending")?.id ?? null;
      if (lastPending.current && lastPending.current !== pendingId) void refreshPlan();
      lastPending.current = pendingId;
    } catch (e) {
      setRequestsError(asCmdError(e).message);
    }
  }, [refreshPlan]);

  useEffect(() => {
    subscriptionApi
      .plans()
      .then(setPlans)
      .catch((e) => setPlansError(asCmdError(e).message));
    void loadRequests();
  }, [loadRequests]);

  const pending = requests?.find((r) => r.status === "pending") ?? null;
  const latest = requests?.[0] ?? null;

  useEffect(() => {
    if (!pending) return;
    const id = window.setInterval(() => void loadRequests(), PENDING_POLL_MS);
    return () => window.clearInterval(id);
  }, [pending, loadRequests]);

  async function refresh() {
    setRefreshing(true);
    await Promise.all([refreshPlan(), loadRequests()]);
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
  const recent = latest?.reviewed_at && Date.now() - new Date(latest.reviewed_at).getTime() < RECENT_MS;

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
            <Item label="Status">
              {info.is_trial ? (info.read_only ? "Free trial ended" : "Free trial") : (STATUS_LABEL[info.status] ?? info.status)}
            </Item>
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

      {pending && <PendingNotice request={pending} onCancel={() => setCancelling(pending)} />}
      {!pending && latest?.status === "rejected" && recent && (
        <Notice tone="red" icon={<XCircle className="size-5" aria-hidden />} title="Your last payment wasn't approved">
          <p>
            {planTermLabel(latest.plan, latest.term)} · {peso(latest.amount_php)} · reference <span className="font-mono">{latest.reference_no}</span>
          </p>
          {latest.review_note && <p className="mt-1">Reason: {latest.review_note}</p>}
          <p className="mt-1">If you did pay, check the reference number and receipt, then send it again below.</p>
        </Notice>
      )}
      {!pending && latest?.status === "approved" && recent && (
        <Notice tone="teal" icon={<CircleCheck className="size-5" aria-hidden />} title="Payment approved">
          <p>
            {planTermLabel(latest.plan, latest.term)} · {peso(latest.amount_php)}
            {latest.start_date && latest.end_date && (
              <>
                {" "}
                · valid {formatDate(latest.start_date)} – {formatDate(latest.end_date)}
              </>
            )}
          </p>
        </Notice>
      )}

      <section aria-labelledby="plans-title" className="space-y-3">
        <div className="flex flex-wrap items-end justify-between gap-3">
          <div>
            <h2 id="plans-title" className="text-base font-semibold">
              Plans
            </h2>
            <p className="text-sm text-slate-500">
              Pay with GCash, then send the receipt. Your plan changes once Clinilog confirms the payment.
            </p>
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
              <PlanCard
                key={p.plan}
                option={p}
                term={term}
                currentPlan={info?.plan}
                currentActive={!!info && !info.read_only}
                trial={!!info?.is_trial}
                blocked={!!pending}
                onChoose={() => setPaying(p)}
              />
            ))}
          </div>
        )}
      </section>

      <section aria-labelledby="history-title" className="space-y-3">
        <h2 id="history-title" className="text-base font-semibold">
          Payment history
        </h2>
        {requestsError && <ErrorNotice>{requestsError}</ErrorNotice>}
        {requests && requests.length === 0 && <p className="text-sm text-slate-500">No payments sent yet.</p>}
        {requests && requests.length > 0 && <History items={requests} />}
      </section>

      {paying && plans && info && (
        <PayDialog
          option={paying}
          term={term}
          payment={plans.payment}
          current={{ plan: info.plan, active: !info.read_only, end: info.end_date, trial: info.is_trial }}
          onClose={() => setPaying(null)}
          onSent={() => void loadRequests()}
        />
      )}
      <CancelDialog request={cancelling} onClose={() => setCancelling(null)} onDone={() => void loadRequests()} />
    </div>
  );
}

function Notice({ tone, icon, title, children }: { tone: "amber" | "red" | "teal"; icon: ReactNode; title: string; children: ReactNode }) {
  const cls = {
    amber: "bg-amber-50 text-amber-950 ring-amber-200 [&>svg]:text-amber-700",
    red: "bg-red-50 text-red-950 ring-red-200",
    teal: "bg-teal-50 text-teal-950 ring-teal-200",
  }[tone];
  return (
    <div role="status" className={`flex gap-3 rounded-xl p-4 text-sm ring-1 ${cls}`}>
      <div className="mt-0.5 shrink-0">{icon}</div>
      <div className="min-w-0 flex-1">
        <p className="font-semibold">{title}</p>
        <div className="mt-1">{children}</div>
      </div>
    </div>
  );
}

function PendingNotice({ request, onCancel }: { request: PaymentRequest; onCancel: () => void }) {
  return (
    <Notice tone="amber" icon={<Clock className="size-5 text-amber-700" aria-hidden />} title="Payment waiting for review">
      <p>
        {planTermLabel(request.plan, request.term)} · {peso(request.amount_php)} · reference{" "}
        <span className="font-mono">{request.reference_no}</span> · sent {formatDateTime(request.created_at)}
      </p>
      <p className="mt-1">
        Clinilog is checking your GCash payment. Your plan changes as soon as it's approved; this page checks every minute.
      </p>
      <button type="button" className={`${buttonClass("secondary", "sm")} mt-3`} onClick={onCancel}>
        Cancel this request
      </button>
    </Notice>
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
  trial,
  blocked,
  onChoose,
}: {
  option: PlanOption;
  term: Term;
  currentPlan?: PlanName;
  currentActive: boolean;
  /** The school is on its free trial. */
  trial: boolean;
  /** A payment is already waiting for review. */
  blocked: boolean;
  onChoose: () => void;
}) {
  const isCurrent = currentPlan === option.plan;
  const premium = option.plan === "premium";
  const price = term === "monthly" ? option.monthly_php : option.yearly_php;
  const action = isCurrent
    ? `${trial ? "Continue with" : "Renew"} ${planLabel(option.plan)}`
    : premium
      ? "Upgrade to Premium"
      : "Switch to Basic";
  const saving = option.monthly_php * 12 - option.yearly_php;

  return (
    <div className={`flex flex-col rounded-xl border bg-white p-5 shadow-sm ${premium ? "border-amber-300 ring-1 ring-amber-200" : "border-slate-200"}`}>
      <div className="flex items-center justify-between gap-2">
        <h3 className="text-lg font-semibold">{planLabel(option.plan)}</h3>
        {isCurrent && (
          <span className="inline-flex items-center gap-1 rounded-full bg-teal-50 px-2 py-0.5 text-xs font-semibold text-teal-800 ring-1 ring-teal-200">
            <CircleCheck className="size-3" aria-hidden />{" "}
            {trial ? (currentActive ? "Free trial" : "Trial ended") : currentActive ? "Current plan" : "Your plan (ended)"}
          </span>
        )}
      </div>
      <p className="mt-2">
        <span className="text-3xl font-semibold tabular-nums">{peso(price)}</span>
        <span className="text-sm text-slate-500"> / {term === "monthly" ? "month" : "year"}</span>
      </p>
      <p className="text-xs text-slate-500">
        {term === "yearly"
          ? saving > 0
            ? `Save ${peso(saving)} compared with paying monthly`
            : "Billed once a year"
          : `${peso(option.yearly_php)} if paid yearly`}
      </p>
      <ul className="mt-4 flex-1 space-y-2 text-sm text-slate-700">
        {FEATURES(option).map((f) => (
          <li key={f} className="flex gap-2">
            <Check className="mt-0.5 size-4 shrink-0 text-teal-700" aria-hidden />
            {f}
          </li>
        ))}
      </ul>
      <button
        type="button"
        className={`${buttonClass(isCurrent ? "secondary" : "primary")} mt-5`}
        onClick={onChoose}
        disabled={blocked}
        title={blocked ? "Wait for the payment under review, or cancel it first." : undefined}
      >
        {action}
      </button>
    </div>
  );
}

/** When the new plan would start if approved today, mirroring the server's rule. */
function startText(option: PlanOption, current: { plan: PlanName; active: boolean; end: string | null; trial: boolean }) {
  if (current.active && current.plan === option.plan && current.end) {
    return current.trial
      ? `Starts when your free trial ends, after ${formatDate(current.end)}`
      : `Adds to your current plan — continues after ${formatDate(current.end)}`;
  }
  if (current.active) {
    return current.trial ? "Starts on approval and ends your free trial" : `Starts on approval and replaces your ${planLabel(current.plan)} plan`;
  }
  return "Starts on approval";
}

function PayDialog({
  option,
  term,
  payment,
  current,
  onClose,
  onSent,
}: {
  option: PlanOption;
  term: Term;
  payment: PaymentInfo;
  current: { plan: PlanName; active: boolean; end: string | null; trial: boolean };
  onClose: () => void;
  onSent: () => void;
}) {
  const [reference, setReference] = useState("");
  const [receipt, setReceipt] = useState<string | null>(null);
  const [reading, setReading] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [fields, setFields] = useState<Record<string, string>>({});
  const [sent, setSent] = useState<PaymentRequest | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);
  const price = term === "monthly" ? option.monthly_php : option.yearly_php;

  async function pickFile(file: File | undefined) {
    if (!file) return;
    setReading(true);
    setFields((f) => ({ ...f, receipt_image: "" }));
    try {
      setReceipt(await readReceiptImage(file));
    } catch (e) {
      setFields((f) => ({ ...f, receipt_image: e instanceof Error ? e.message : "Couldn't open that image." }));
    } finally {
      setReading(false);
      if (fileRef.current) fileRef.current.value = "";
    }
  }

  async function send() {
    const errs: Record<string, string> = {};
    const ref = reference.replace(/[\s-]/g, "");
    if (!ref) errs.reference_no = "Enter the reference number from your GCash receipt.";
    else if (!/^[A-Za-z0-9]{6,30}$/.test(ref)) errs.reference_no = "The reference number should be 6–30 letters or numbers, as shown on the receipt.";
    if (!receipt) errs.receipt_image = "Upload a screenshot or photo of your GCash receipt.";
    setFields(errs);
    if (Object.keys(errs).length) return;
    setBusy(true);
    setError(null);
    try {
      const r = await subscriptionApi.submit({ plan: option.plan, term, reference_no: reference, receipt_image: receipt! });
      setSent(r);
      onSent();
    } catch (e) {
      const err = asCmdError(e);
      if (err.fields) setFields(err.fields);
      else setError(err.message);
      if (err.code === "request_pending") onSent();
    } finally {
      setBusy(false);
    }
  }

  if (sent) {
    return (
      <Dialog
        open
        onClose={onClose}
        title="Payment sent for review"
        footer={
          <button type="button" className={buttonClass("primary")} onClick={onClose} autoFocus>
            Done
          </button>
        }
      >
        <div className="space-y-3">
          <p className="flex items-center gap-2 font-medium text-amber-800">
            <Clock className="size-5" aria-hidden /> Waiting for Clinilog to confirm your payment.
          </p>
          <dl className="grid grid-cols-2 gap-x-4 gap-y-2 rounded-lg bg-slate-50 p-3 text-sm">
            <dt className="text-slate-500">Plan</dt>
            <dd>{planTermLabel(sent.plan, sent.term)}</dd>
            <dt className="text-slate-500">Amount</dt>
            <dd>{peso(sent.amount_php)}</dd>
            <dt className="text-slate-500">Reference</dt>
            <dd className="font-mono">{sent.reference_no}</dd>
            <dt className="text-slate-500">Sent</dt>
            <dd>{formatDateTime(sent.created_at)}</dd>
          </dl>
          <p>Your plan stays as it is until the payment is approved. You can see the status on this page.</p>
        </div>
      </Dialog>
    );
  }

  return (
    <Dialog
      open
      wide
      onClose={() => !busy && onClose()}
      title={`${current.plan === option.plan && !current.trial ? "Renew" : "Get"} ${planLabel(option.plan)} — ${term === "monthly" ? "monthly" : "yearly"}`}
      footer={
        <>
          <button type="button" className={buttonClass("secondary")} onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className={buttonClass("primary")} onClick={() => void send()} disabled={busy || reading}>
            {busy && <Loader2 className="size-4 animate-spin" aria-hidden />}
            Send for review
          </button>
        </>
      }
    >
      <div className="grid gap-6 md:grid-cols-[220px_1fr]">
        <div className="text-center">
          <img
            src={payment.qr_image}
            alt={`GCash InstaPay QR code for ${payment.account_name}`}
            className="mx-auto w-[220px] rounded-lg ring-1 ring-slate-200"
            draggable={false}
          />
          <p className="mt-2 text-xs text-slate-500">
            {payment.label} · {payment.account_name}
          </p>
        </div>

        <div className="space-y-4">
          <div className="rounded-lg bg-slate-50 p-3">
            <p className="text-xs font-medium text-slate-500">Amount to pay</p>
            <p className="text-2xl font-semibold tabular-nums">{peso(price)}</p>
            <p className="text-xs text-slate-500">
              {planTermLabel(option.plan, term)} · {startText(option, current)}
            </p>
          </div>

          <ol className="list-decimal space-y-1 pl-5 text-sm text-slate-700">
            <li>
              Open GCash on your phone, tap <strong>QR</strong>, and scan the code.
            </li>
            <li>
              Pay exactly <strong>{peso(price)}</strong>.
            </li>
            <li>Take a screenshot of the receipt. It shows the reference number.</li>
            <li>Enter the reference number and upload the screenshot below.</li>
          </ol>

          <TextField
            label="GCash reference number"
            value={reference}
            onChange={(e) => {
              setReference(e.target.value);
              setFields((f) => ({ ...f, reference_no: "" }));
            }}
            error={fields.reference_no}
            hint="Shown on the receipt as Ref No., e.g. 1012 345 678 901"
            autoComplete="off"
            spellCheck={false}
            maxLength={40}
            className="font-mono"
          />

          <Field label="Receipt screenshot" error={fields.receipt_image}>
            {({ id, describedBy, invalid }) => (
              <div className="mt-1">
                <input
                  ref={fileRef}
                  id={id}
                  type="file"
                  accept="image/png,image/jpeg,image/webp"
                  className="sr-only"
                  aria-describedby={describedBy}
                  aria-invalid={invalid}
                  onChange={(e) => void pickFile(e.target.files?.[0])}
                />
                {receipt ? (
                  <div className="flex items-center gap-3">
                    <img src={receipt} alt="Receipt to send" className="h-28 rounded-md object-contain ring-1 ring-slate-200" />
                    <button type="button" className={buttonClass("secondary", "sm")} onClick={() => fileRef.current?.click()} disabled={reading}>
                      Choose another
                    </button>
                  </div>
                ) : (
                  <button
                    type="button"
                    onClick={() => fileRef.current?.click()}
                    disabled={reading}
                    className={inputClass(invalid, "flex items-center justify-center gap-2 border-dashed py-5 text-slate-600 hover:bg-slate-50")}
                  >
                    {reading ? <Loader2 className="size-4 animate-spin" aria-hidden /> : <ImageUp className="size-4" aria-hidden />}
                    {reading ? "Opening…" : "Choose image (PNG, JPG or WEBP)"}
                  </button>
                )}
              </div>
            )}
          </Field>

          {error && <ErrorNotice>{error}</ErrorNotice>}
        </div>
      </div>
    </Dialog>
  );
}

function CancelDialog({ request, onClose, onDone }: { request: PaymentRequest | null; onClose: () => void; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <ConfirmDialog
      open={!!request}
      title="Cancel this payment request?"
      confirmLabel="Cancel request"
      cancelLabel="Keep it"
      busy={busy}
      error={error}
      onCancel={() => {
        setError(null);
        onClose();
      }}
      onConfirm={async () => {
        if (!request) return;
        setBusy(true);
        setError(null);
        try {
          await subscriptionApi.cancel(request.id);
          onDone();
          onClose();
        } catch (e) {
          setError(asCmdError(e).message);
          onDone();
        } finally {
          setBusy(false);
        }
      }}
    >
      <p>Clinilog won't review it. Only cancel if you sent the wrong reference number or receipt, or didn't actually pay.</p>
      <p>If you already paid, you can send the correct details again after cancelling.</p>
    </ConfirmDialog>
  );
}

function History({ items }: { items: PaymentRequest[] }) {
  return (
    <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
      <table className="w-full text-left text-sm">
        <thead className="border-b border-slate-200 bg-slate-50 text-xs font-semibold tracking-wide text-slate-500 uppercase">
          <tr>
            <th scope="col" className="px-4 py-3">Sent</th>
            <th scope="col" className="px-4 py-3">Plan</th>
            <th scope="col" className="px-4 py-3 text-right">Amount</th>
            <th scope="col" className="px-4 py-3">Reference</th>
            <th scope="col" className="px-4 py-3">Status</th>
            <th scope="col" className="px-4 py-3">Details</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-slate-100">
          {items.map((r) => (
            <tr key={r.id}>
              <td className="px-4 py-3 whitespace-nowrap text-slate-600">
                {formatDateTime(r.created_at)}
                <span className="block text-xs text-slate-400">by {r.requested_by_name}</span>
              </td>
              <td className="px-4 py-3">{planTermLabel(r.plan, r.term)}</td>
              <td className="px-4 py-3 text-right tabular-nums">{peso(r.amount_php)}</td>
              <td className="px-4 py-3 font-mono text-slate-600">{r.reference_no}</td>
              <td className="px-4 py-3">
                <RequestStatusBadge status={r.status} />
              </td>
              <td className="px-4 py-3 text-slate-600">
                {r.status === "approved" && r.start_date && r.end_date && (
                  <>
                    {formatDate(r.start_date)} – {formatDate(r.end_date)}
                  </>
                )}
                {r.status === "rejected" && r.review_note}
                {r.status === "approved" && r.review_note && <span className="block text-xs text-slate-500">{r.review_note}</span>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
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
