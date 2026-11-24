import { AlertTriangle, ArrowLeft, CircleCheck, Loader2, Maximize2, XCircle } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { Link, useParams } from "react-router-dom";
import { Dialog } from "../../components/Dialog";
import { buttonClass, ErrorNotice, Field, inputClass } from "../../components/form";
import { planTermLabel, RequestStatusBadge } from "../../components/payments";
import { Card } from "../../components/ui";
import { asCmdError, superadminApi, type ReviewDetail } from "../../lib/api";
import { formatDate, formatDateTime, peso, planLabel } from "../../lib/format";

const REJECT_REASONS = [
  "No payment was received with this reference number.",
  "The amount received doesn't match the plan price.",
  "The receipt is unclear or doesn't match the reference number.",
];

/** What approving will do to the school's plan, mirroring the server's rule. */
function effectText(d: ReviewDetail): string {
  const cur = d.current_subscription;
  const active = !cur.read_only;
  const months = d.term === "monthly" ? "1 month" : "1 year";
  if (active && cur.plan === d.plan && cur.end_date) {
    return `Extends their ${planLabel(cur.plan)} plan by ${months}, continuing after ${formatDate(cur.end_date)}.`;
  }
  if (active) return `Starts ${planLabel(d.plan)} today for ${months} and ends their current ${planLabel(cur.plan)} plan.`;
  return `Starts ${planLabel(d.plan)} today for ${months}.`;
}

export function ReviewPaymentPage() {
  const { id = "" } = useParams();
  const [detail, setDetail] = useState<ReviewDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [zoom, setZoom] = useState(false);
  const [deciding, setDeciding] = useState<"approve" | "reject" | null>(null);

  const load = useCallback(async () => {
    try {
      setDetail(await superadminApi.request(id));
      setError(null);
    } catch (e) {
      setError(asCmdError(e).message);
    }
  }, [id]);

  useEffect(() => {
    void load();
  }, [load]);

  const back = (
    <Link to="/" className="inline-flex items-center gap-1 text-sm font-medium text-teal-700 hover:underline">
      <ArrowLeft className="size-4" aria-hidden /> All payment requests
    </Link>
  );

  if (error && !detail) {
    return (
      <div className="max-w-5xl space-y-4">
        {back}
        <ErrorNotice>{error}</ErrorNotice>
      </div>
    );
  }
  if (!detail) {
    return (
      <p className="flex items-center gap-2 text-sm text-slate-500">
        <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
      </p>
    );
  }

  const cur = detail.current_subscription;
  const pending = detail.status === "pending";

  return (
    <div className="max-w-5xl space-y-4">
      {back}
      <div className="grid gap-6 lg:grid-cols-[minmax(0,5fr)_minmax(0,6fr)]">
        <Card
          title="Receipt sent by the school"
          action={
            <button type="button" className={buttonClass("ghost", "sm")} onClick={() => setZoom(true)}>
              <Maximize2 className="size-3.5" aria-hidden /> Full size
            </button>
          }
        >
          <button type="button" onClick={() => setZoom(true)} className="block w-full" aria-label="Show the receipt full size">
            <img src={detail.receipt_image} alt="GCash receipt uploaded by the school" className="mx-auto max-h-[560px] rounded-md object-contain ring-1 ring-slate-200" />
          </button>
        </Card>

        <div className="space-y-4">
          <Card title="Payment" action={<RequestStatusBadge status={detail.status} />}>
            <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2.5 text-sm">
              <Row label="Reference no.">
                <span className="font-mono text-base font-semibold tracking-wide">{detail.reference_no}</span>
              </Row>
              <Row label="Amount due">
                <span className="text-base font-semibold">{peso(detail.amount_php)}</span>
              </Row>
              <Row label="Plan">{planTermLabel(detail.plan, detail.term)}</Row>
              <Row label="Method">GCash (InstaPay QR)</Row>
              <Row label="School">{detail.school_name}</Row>
              <Row label="Sent by">
                {detail.requested_by_name} <span className="text-slate-500">({detail.requested_by_username})</span>
              </Row>
              <Row label="Sent">{formatDateTime(detail.created_at)}</Row>
              {detail.reviewed_at && <Row label={detail.status === "cancelled" ? "Cancelled" : "Reviewed"}>{formatDateTime(detail.reviewed_at)}</Row>}
              {detail.review_note && <Row label="Note">{detail.review_note}</Row>}
              {detail.status === "approved" && detail.start_date && detail.end_date && (
                <Row label="Plan period">
                  {formatDate(detail.start_date)} – {formatDate(detail.end_date)}
                </Row>
              )}
            </dl>
            {detail.reference_uses_elsewhere > 0 && (
              <p className="mt-4 flex gap-2 rounded-lg bg-amber-50 p-3 text-sm text-amber-900 ring-1 ring-amber-200">
                <AlertTriangle className="mt-0.5 size-4 shrink-0" aria-hidden />
                This reference number was also used in {detail.reference_uses_elsewhere} other request
                {detail.reference_uses_elsewhere === 1 ? "" : "s"}. Check it carefully.
              </p>
            )}
          </Card>

          <Card title="School's current plan">
            <dl className="grid grid-cols-[auto_1fr] gap-x-6 gap-y-2.5 text-sm">
              <Row label="Plan">{cur.status === "none" ? "None" : planLabel(cur.plan)}</Row>
              <Row label="Status">{cur.read_only ? (cur.status === "none" ? "No subscription" : "Ended") : "Active"}</Row>
              <Row label="Valid until">{cur.end_date ? formatDate(cur.end_date) : "—"}</Row>
            </dl>
            {pending && <p className="mt-3 text-sm text-slate-600">If approved: {effectText(detail)}</p>}
          </Card>

          {pending && (
            <Card title="Decision">
              <ol className="list-decimal space-y-1 pl-5 text-sm text-slate-700">
                <li>
                  Open your GCash transaction history and find reference <span className="font-mono font-semibold">{detail.reference_no}</span>.
                </li>
                <li>
                  Check that you received <strong>{peso(detail.amount_php)}</strong> and that it matches the receipt.
                </li>
              </ol>
              {error && (
                <div className="mt-3">
                  <ErrorNotice>{error}</ErrorNotice>
                </div>
              )}
              <div className="mt-4 flex flex-wrap gap-2">
                <button type="button" className={buttonClass("primary")} onClick={() => setDeciding("approve")}>
                  <CircleCheck className="size-4" aria-hidden /> Approve payment
                </button>
                <button type="button" className={buttonClass("danger")} onClick={() => setDeciding("reject")}>
                  <XCircle className="size-4" aria-hidden /> Reject
                </button>
              </div>
            </Card>
          )}
        </div>
      </div>

      <Dialog open={zoom} onClose={() => setZoom(false)} title={`Receipt · ref ${detail.reference_no}`} wide>
        <img src={detail.receipt_image} alt="GCash receipt, full size" className="mx-auto max-w-full" />
      </Dialog>

      {deciding && (
        <DecisionDialog
          kind={deciding}
          detail={detail}
          onClose={() => setDeciding(null)}
          onDone={() => {
            setDeciding(null);
            void load();
          }}
        />
      )}
    </div>
  );
}

function DecisionDialog({
  kind,
  detail,
  onClose,
  onDone,
}: {
  kind: "approve" | "reject";
  detail: ReviewDetail;
  onClose: () => void;
  onDone: () => void;
}) {
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [noteError, setNoteError] = useState<string | undefined>();
  const approve = kind === "approve";

  async function submit() {
    if (!approve && !note.trim()) {
      setNoteError("Tell the school why, so they can fix it.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      if (approve) await superadminApi.approve(detail.id, note.trim());
      else await superadminApi.reject(detail.id, note.trim());
      onDone();
    } catch (e) {
      const err = asCmdError(e);
      if (err.fields?.note) setNoteError(err.fields.note);
      else setError(err.message);
      if (err.code === "already_reviewed") onDone();
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open
      onClose={() => !busy && onClose()}
      title={approve ? "Approve this payment?" : "Reject this payment?"}
      footer={
        <>
          <button type="button" className={buttonClass("secondary")} onClick={onClose} disabled={busy}>
            Back
          </button>
          <button type="button" className={buttonClass(approve ? "primary" : "danger")} onClick={() => void submit()} disabled={busy}>
            {busy && <Loader2 className="size-4 animate-spin" aria-hidden />}
            {approve ? "Approve" : "Reject payment"}
          </button>
        </>
      }
    >
      <div className="space-y-3">
        {approve ? (
          <p>
            {peso(detail.amount_php)} from {detail.school_name}, reference <span className="font-mono">{detail.reference_no}</span>.{" "}
            {effectText(detail)}
          </p>
        ) : (
          <>
            <p>The school sees your reason and can send the payment again.</p>
            <div className="flex flex-wrap gap-1.5">
              {REJECT_REASONS.map((r) => (
                <button
                  key={r}
                  type="button"
                  onClick={() => {
                    setNote(r);
                    setNoteError(undefined);
                  }}
                  className="rounded-full bg-slate-100 px-2.5 py-1 text-left text-xs text-slate-700 hover:bg-slate-200"
                >
                  {r}
                </button>
              ))}
            </div>
          </>
        )}
        <Field label={approve ? "Note" : "Reason"} optional={approve} error={noteError}>
          {({ id, describedBy, invalid }) => (
            <textarea
              id={id}
              rows={3}
              maxLength={500}
              value={note}
              onChange={(e) => {
                setNote(e.target.value);
                setNoteError(undefined);
              }}
              aria-describedby={describedBy}
              aria-invalid={invalid}
              className={inputClass(invalid)}
            />
          )}
        </Field>
        {error && <ErrorNotice>{error}</ErrorNotice>}
      </div>
    </Dialog>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <dt className="text-slate-500">{label}</dt>
      <dd className="min-w-0 break-words text-slate-900">{children}</dd>
    </>
  );
}
