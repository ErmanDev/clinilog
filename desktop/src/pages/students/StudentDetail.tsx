import { ArrowLeft, Loader2, Pencil, Plus, Stethoscope, Trash2 } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ConfirmDialog } from "../../components/Dialog";
import { buttonClass, ErrorNotice } from "../../components/form";
import { Card } from "../../components/ui";
import { asCmdError, studentsApi, type StudentDetail } from "../../lib/api";
import { formatDate, formatDateTime, fullName, genderLabel, sectionLabel } from "../../lib/format";
import { usePlan } from "../../lib/session";

export function StudentDetailPage() {
  const { id } = useParams();
  const navigate = useNavigate();
  const { readOnly, isAdmin } = usePlan();
  const [student, setStudent] = useState<StudentDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  useEffect(() => {
    if (!id) return;
    setStudent(null);
    studentsApi
      .get(id)
      .then(setStudent)
      .catch((e) => setError(asCmdError(e).message));
  }, [id]);

  async function onDelete() {
    if (!student) return;
    setDeleting(true);
    setDeleteError(null);
    try {
      await studentsApi.remove(student.id);
      navigate("/students", { replace: true });
    } catch (e) {
      setDeleteError(asCmdError(e).message);
    } finally {
      setDeleting(false);
    }
  }

  const back = (
    <Link to="/students" className="inline-flex items-center gap-1 text-sm text-slate-600 hover:text-slate-900">
      <ArrowLeft className="size-4" aria-hidden /> All students
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
  if (!student) {
    return (
      <div className="flex items-center gap-2 text-sm text-slate-500">
        <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
      </div>
    );
  }

  const name = fullName(student);

  return (
    <div className="space-y-5">
      {back}

      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h2 className="text-2xl font-semibold tracking-tight">{name}</h2>
          <p className="mt-1 text-sm text-slate-500">
            LRN <span className="font-mono text-slate-700 tabular-nums">{student.lrn}</span> ·{" "}
            {sectionLabel(student.section)}
          </p>
        </div>
        {!readOnly && (
          <div className="flex gap-2">
            <Link to={`/visits/new?student=${student.id}`} className={buttonClass("primary")}>
              <Plus className="size-4" aria-hidden /> Record visit
            </Link>
            <Link to={`/students/${student.id}/edit`} className={buttonClass("secondary")}>
              <Pencil className="size-4" aria-hidden /> Edit
            </Link>
            {isAdmin && (
              <button
                type="button"
                className={buttonClass("secondary")}
                onClick={() => {
                  setDeleteError(null);
                  setConfirmDelete(true);
                }}
              >
                <Trash2 className="size-4 text-red-600" aria-hidden /> Delete
              </button>
            )}
          </div>
        )}
      </div>

      <Card title="Profile">
        <dl className="grid grid-cols-2 gap-x-6 gap-y-4 text-sm md:grid-cols-4">
          <Item label="First name">{student.first_name}</Item>
          <Item label="Middle name">{student.middle_name ?? "—"}</Item>
          <Item label="Last name">{student.last_name}</Item>
          <Item label="Gender">{genderLabel(student.gender)}</Item>
          <Item label="Birthdate">{formatDate(student.birthdate)}</Item>
          <Item label="Age">{student.age}</Item>
          <Item label="Grade & section">{sectionLabel(student.section)}</Item>
          <Item label="Last updated">{formatDateTime(student.updated_at)}</Item>
        </dl>
      </Card>

      <Card title={`Visit history (${student.visits.length})`}>
        {student.visits.length === 0 ? (
          <div className="flex flex-col items-center py-8 text-center">
            <Stethoscope className="size-7 text-slate-300" aria-hidden />
            <p className="mt-2 text-sm text-slate-600">No clinic visits recorded yet.</p>
          </div>
        ) : (
          <div className="-mx-5 -mb-5 overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead className="border-y border-slate-200 bg-slate-50 text-xs font-semibold tracking-wide text-slate-500 uppercase">
                <tr>
                  <th scope="col" className="px-5 py-2.5">Date &amp; time</th>
                  <th scope="col" className="px-5 py-2.5">Complaint</th>
                  <th scope="col" className="px-5 py-2.5">Treatment</th>
                  <th scope="col" className="px-5 py-2.5">Medicines</th>
                  <th scope="col" className="px-5 py-2.5">Attended by</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100 align-top">
                {student.visits.map((v) => (
                  <tr key={v.id} onClick={() => navigate(`/visits/${v.id}`)} className="cursor-pointer hover:bg-teal-50/50">
                    <td className="px-5 py-3 whitespace-nowrap text-slate-600">
                      <Link to={`/visits/${v.id}`} onClick={(e) => e.stopPropagation()} className="outline-none hover:underline focus-visible:underline">
                        {formatDateTime(v.visited_at)}
                      </Link>
                    </td>
                    <td className="px-5 py-3 whitespace-pre-wrap">{v.complaint}</td>
                    <td className="px-5 py-3 whitespace-pre-wrap">{v.treatment}</td>
                    <td className="px-5 py-3 text-slate-600">
                      {v.medicines.length === 0
                        ? "—"
                        : v.medicines.map((m) => `${m.name} × ${m.quantity} ${m.unit}`).join(", ")}
                    </td>
                    <td className="px-5 py-3 text-slate-600">{v.attended_by}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>

      <ConfirmDialog
        open={confirmDelete}
        title="Delete student?"
        confirmLabel="Delete student"
        busy={deleting}
        error={deleteError}
        onCancel={() => setConfirmDelete(false)}
        onConfirm={() => void onDelete()}
      >
        <p>
          <strong className="font-semibold">{name}</strong> (LRN <span className="font-mono">{student.lrn}</span>) will
          be permanently removed. This can't be undone.
        </p>
      </ConfirmDialog>
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
