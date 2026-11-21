import { ArrowLeft, Loader2, Pencil, Trash2 } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ConfirmDialog } from "../../components/Dialog";
import { buttonClass, ErrorNotice } from "../../components/form";
import { Card } from "../../components/ui";
import { asCmdError, visitsApi, type Visit } from "../../lib/api";
import { formatDateTime, fullName, sectionLabel } from "../../lib/format";
import { usePlan, useSession } from "../../lib/session";

export function VisitDetailPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { session } = useSession();
  const { readOnly, isAdmin } = usePlan();
  const [visit, setVisit] = useState<Visit | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    visitsApi
      .get(id)
      .then(setVisit)
      .catch((e) => setError(asCmdError(e).message));
  }, [id]);

  const back = (
    <Link to="/visits" className="inline-flex items-center gap-1 text-sm text-slate-600 hover:text-slate-900">
      <ArrowLeft className="size-4" aria-hidden /> All visits
    </Link>
  );

  if (error) {
    return (
      <div className="space-y-4">
        {back}
        <ErrorNotice>{error}</ErrorNotice>
      </div>
    );
  }
  if (!visit) {
    return (
      <p className="flex items-center gap-2 text-sm text-slate-500">
        <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
      </p>
    );
  }

  // Mirrors the server rule: the recorder or an admin may edit.
  const canEdit = !readOnly && (isAdmin || visit.attended_by.id === session?.user.id);

  async function onDelete() {
    setDeleting(true);
    setDeleteError(null);
    try {
      await visitsApi.remove(visit!.id);
      navigate("/visits", { replace: true });
    } catch (e) {
      setDeleteError(asCmdError(e).message);
    } finally {
      setDeleting(false);
    }
  }

  return (
    <div className="max-w-3xl space-y-5">
      {back}

      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 className="text-2xl font-semibold tracking-tight">Clinic visit</h2>
          <p className="mt-1 text-sm text-slate-500">
            {formatDateTime(visit.visited_at)} · attended by {visit.attended_by.full_name}
          </p>
        </div>
        <div className="flex gap-2">
          {canEdit && (
            <Link to={`/visits/${visit.id}/edit`} className={buttonClass("secondary")}>
              <Pencil className="size-4" aria-hidden /> Edit
            </Link>
          )}
          {isAdmin && !readOnly && (
            <button type="button" className={buttonClass("secondary")} onClick={() => setConfirmDelete(true)}>
              <Trash2 className="size-4 text-red-600" aria-hidden /> Delete
            </button>
          )}
        </div>
      </div>

      <Card title="Student">
        <Link to={`/students/${visit.student.id}`} className="text-base font-semibold text-teal-800 hover:underline">
          {fullName(visit.student)}
        </Link>
        <p className="mt-0.5 text-sm text-slate-500">
          LRN <span className="font-mono">{visit.student.lrn}</span> · {sectionLabel(visit.student)}
        </p>
      </Card>

      <Card title="Details">
        <dl className="space-y-4 text-sm">
          <Item label="Complaint">{visit.complaint}</Item>
          <Item label="Treatment">{visit.treatment}</Item>
          <Item label="Medicines given">
            {visit.medicines.length === 0 ? (
              "None"
            ) : (
              <ul className="list-inside list-disc">
                {visit.medicines.map((m) => (
                  <li key={m.medicine_id}>
                    {m.name} — {m.quantity} {m.unit}
                  </li>
                ))}
              </ul>
            )}
          </Item>
        </dl>
      </Card>

      <ConfirmDialog
        open={confirmDelete}
        title="Delete visit?"
        confirmLabel="Delete visit"
        busy={deleting}
        error={deleteError}
        onCancel={() => setConfirmDelete(false)}
        onConfirm={() => void onDelete()}
      >
        <p>
          The visit of <strong className="font-semibold">{fullName(visit.student)}</strong> on{" "}
          {formatDateTime(visit.visited_at)} will be permanently removed. This can't be undone.
        </p>
      </ConfirmDialog>
    </div>
  );
}

function Item({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div>
      <dt className="text-xs font-medium text-slate-500">{label}</dt>
      <dd className="mt-0.5 whitespace-pre-wrap text-slate-900">{children}</dd>
    </div>
  );
}
