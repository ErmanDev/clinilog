import { Clock, Loader2, Plus, Trash2, UserRound } from "lucide-react";
import { useEffect, useId, useState, type FormEvent } from "react";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { buttonClass, ErrorNotice, inputClass } from "../../components/form";
import { StudentPicker, toPicked, type PickedStudent } from "../../components/StudentPicker";
import { Card } from "../../components/ui";
import { asCmdError, medicinesApi, studentsApi, syncApi, visitsApi, type Medicine, type VisitInput } from "../../lib/api";
import { formatDateTime, listName } from "../../lib/format";
import { usePlan, useSession } from "../../lib/session";
import { validateQuantity, validateText } from "../../lib/validation";

interface Line {
  key: string;
  medicine_id: string;
  quantity: string;
}

const MAX_LINES = 20;
const newLine = (): Line => ({ key: crypto.randomUUID(), medicine_id: "", quantity: "1" });

/** `/visits/new` (optionally `?student=<id>`) and `/visits/:id/edit` */
export function VisitFormPage() {
  const { id } = useParams();
  const editing = !!id;
  const [params] = useSearchParams();
  const navigate = useNavigate();
  const { session } = useSession();
  const { readOnly } = usePlan();

  const [student, setStudent] = useState<PickedStudent | null>(null);
  const [complaint, setComplaint] = useState("");
  const [treatment, setTreatment] = useState("");
  const [lines, setLines] = useState<Line[]>([]);
  const [medicines, setMedicines] = useState<Medicine[] | null>(null);
  const [recorded, setRecorded] = useState<{ at: string; by: string } | null>(null);

  const [loaded, setLoaded] = useState(!editing);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [touched, setTouched] = useState<Record<string, boolean>>({});
  const [serverErrors, setServerErrors] = useState<Record<string, string>>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [now, setNow] = useState(() => new Date().toISOString());

  const complaintId = useId();
  const treatmentId = useId();

  // Offline, these come from the records saved on this PC (the app does this by itself).
  useEffect(() => {
    medicinesApi
      .list()
      .then(setMedicines)
      .catch((e) => setFormError(`Couldn't load medicines: ${asCmdError(e).message}`));
  }, []);

  // Live clock for the "recorded at" line on new visits.
  useEffect(() => {
    if (editing) return;
    const t = window.setInterval(() => setNow(new Date().toISOString()), 15_000);
    return () => window.clearInterval(t);
  }, [editing]);

  // Pre-fill the student when coming from a student's page.
  useEffect(() => {
    const sid = params.get("student");
    if (editing || !sid) return;
    studentsApi
      .get(sid)
      .then((s) => setStudent(toPicked(s)))
      .catch(() => {});
  }, [editing, params]);

  useEffect(() => {
    if (!id) return;
    visitsApi
      .get(id)
      .then((v) => {
        setStudent({ ...v.student });
        setComplaint(v.complaint);
        setTreatment(v.treatment);
        setLines(v.medicines.map((m) => ({ key: crypto.randomUUID(), medicine_id: m.medicine_id, quantity: String(m.quantity) })));
        setRecorded({ at: v.visited_at, by: v.attended_by.full_name });
        setLoaded(true);
      })
      .catch((e) => setLoadError(asCmdError(e).message));
  }, [id]);

  // ---- validation ----
  const errors: Record<string, string> = {};
  if (!student) errors.student_id = "Choose a student.";
  const ce = validateText(complaint, "complaint", 3, 500);
  if (ce) errors.complaint = ce;
  const te = validateText(treatment, "treatment", 1, 500);
  if (te) errors.treatment = te;
  const seen = new Set<string>();
  lines.forEach((l, i) => {
    if (!l.medicine_id) errors[`medicines.${i}.medicine_id`] = "Choose a medicine.";
    else if (seen.has(l.medicine_id)) errors[`medicines.${i}.medicine_id`] = "This medicine is already listed.";
    seen.add(l.medicine_id);
    const qe = validateQuantity(l.quantity);
    if (qe) errors[`medicines.${i}.quantity`] = qe;
  });
  const valid = Object.keys(errors).length === 0;
  const shown = (k: string) => serverErrors[k] ?? (touched[k] ? errors[k] : undefined) ?? null;
  const touch = (k: string) => setTouched((t) => ({ ...t, [k]: true }));
  const clearServer = (k: string) => setServerErrors((e) => ({ ...e, [k]: "" }));

  function updateLine(i: number, patch: Partial<Line>) {
    setLines((ls) => ls.map((l, j) => (j === i ? { ...l, ...patch } : l)));
    // Line indexes shift on remove, so drop all line-level server messages on any change.
    setServerErrors((e) => Object.fromEntries(Object.entries(e).filter(([k]) => !k.startsWith("medicines."))));
  }

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (!valid || saving || !student) return;
    setSaving(true);
    setFormError(null);
    const medicineLines = lines.map((l) => ({ medicine_id: l.medicine_id, quantity: Number(l.quantity) }));
    try {
      if (editing) {
        const body: VisitInput = { student_id: student.id, complaint, treatment, medicines: medicineLines };
        const saved = await visitsApi.update(id!, body);
        navigate(`/visits/${saved.id}`, { replace: true });
        return;
      }
      // Uploaded right away when online; otherwise saved on this PC and uploaded later.
      const r = await syncApi.recordVisit({
        student_id: student.id,
        complaint,
        treatment,
        medicines: medicineLines,
      });
      if (r.status === "synced") {
        navigate(`/visits/${r.visit_id}`, { replace: true });
      } else {
        navigate("/visits", {
          replace: true,
          state: { notice: `Visit of ${listName(student)} saved on this PC. It will upload automatically when the connection is back.` },
        });
      }
    } catch (err) {
      const ce2 = asCmdError(err);
      if (ce2.fields) setServerErrors(ce2.fields);
      setFormError(ce2.message);
    } finally {
      setSaving(false);
    }
  }

  if (loadError) return <ErrorNotice>{loadError}</ErrorNotice>;
  if (!loaded) {
    return (
      <p className="flex items-center gap-2 text-sm text-slate-500">
        <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
      </p>
    );
  }
  if (readOnly) {
    return (
      <ErrorNotice>
        Visits can't be recorded or changed while the subscription is inactive.{" "}
        <Link to="/visits" className="font-semibold underline">
          Back to visits
        </Link>
      </ErrorNotice>
    );
  }

  const textareaClass = (k: string) => inputClass(!!shown(k), "min-h-24 resize-y");
  const counter = (v: string, max: number) => `${[...v.trim()].length}/${max}`;

  return (
    <form onSubmit={onSubmit} noValidate className="max-w-3xl space-y-4">
      <h2 className="text-base font-semibold">{editing ? "Edit visit" : "New clinic visit"}</h2>
      {formError && <ErrorNotice>{formError}</ErrorNotice>}

      <Card>
        <div className="space-y-5">
          <StudentPicker
            value={student}
            onChange={(s) => {
              setStudent(s);
              touch("student_id");
              clearServer("student_id");
            }}
            error={shown("student_id")}
            disabled={editing}
            autoFocus={!editing && !params.get("student")}
          />

          <div className="flex flex-wrap gap-x-6 gap-y-1 rounded-lg bg-slate-50 px-3 py-2 text-xs text-slate-600">
            <span className="inline-flex items-center gap-1.5">
              <Clock className="size-3.5" aria-hidden />
              {recorded ? formatDateTime(recorded.at) : `${formatDateTime(now)} (set when you save)`}
            </span>
            <span className="inline-flex items-center gap-1.5">
              <UserRound className="size-3.5" aria-hidden />
              Attended by {recorded ? recorded.by : session?.user.full_name}
            </span>
          </div>

          <div>
            <label htmlFor={complaintId} className="block text-sm font-medium text-slate-700">
              Complaint
            </label>
            <textarea
              id={complaintId}
              value={complaint}
              maxLength={500}
              onChange={(e) => {
                setComplaint(e.target.value);
                touch("complaint");
                clearServer("complaint");
              }}
              onBlur={() => touch("complaint")}
              aria-invalid={!!shown("complaint")}
              aria-describedby={`${complaintId}-msg`}
              placeholder="What the student says is wrong, e.g. headache since morning"
              className={textareaClass("complaint")}
              autoFocus={!editing && !!params.get("student")}
            />
            <FieldMsg id={`${complaintId}-msg`} error={shown("complaint")} hint={counter(complaint, 500)} />
          </div>

          <div>
            <label htmlFor={treatmentId} className="block text-sm font-medium text-slate-700">
              Treatment
            </label>
            <textarea
              id={treatmentId}
              value={treatment}
              maxLength={500}
              onChange={(e) => {
                setTreatment(e.target.value);
                touch("treatment");
                clearServer("treatment");
              }}
              onBlur={() => touch("treatment")}
              aria-invalid={!!shown("treatment")}
              aria-describedby={`${treatmentId}-msg`}
              placeholder="What was done, e.g. rested in clinic for 30 minutes"
              className={textareaClass("treatment")}
            />
            <FieldMsg id={`${treatmentId}-msg`} error={shown("treatment")} hint={counter(treatment, 500)} />
          </div>

          <fieldset>
            <legend className="text-sm font-medium text-slate-700">
              Medicines given <span className="font-normal text-slate-400">(optional)</span>
            </legend>
            {lines.length > 0 && (
              <ul className="mt-2 space-y-2">
                {lines.map((l, i) => {
                  const medErr = shown(`medicines.${i}.medicine_id`);
                  const qtyErr = shown(`medicines.${i}.quantity`);
                  const unit = medicines?.find((m) => m.id === l.medicine_id)?.unit;
                  return (
                    <li key={l.key}>
                      <div className="flex items-start gap-2">
                        <div className="flex-1">
                          <label htmlFor={`${l.key}-m`} className="sr-only">
                            Medicine {i + 1}
                          </label>
                          <select
                            id={`${l.key}-m`}
                            value={l.medicine_id}
                            onChange={(e) => {
                              updateLine(i, { medicine_id: e.target.value });
                              touch(`medicines.${i}.medicine_id`);
                            }}
                            aria-invalid={!!medErr}
                            className={inputClass(!!medErr, "mt-0")}
                          >
                            <option value="" disabled>
                              {medicines ? "Choose a medicine…" : "Loading…"}
                            </option>
                            {medicines?.map((m) => (
                              <option key={m.id} value={m.id}>
                                {m.name}
                              </option>
                            ))}
                          </select>
                        </div>
                        <div className="w-36">
                          <label htmlFor={`${l.key}-q`} className="sr-only">
                            Quantity for medicine {i + 1}
                          </label>
                          <div className="flex items-center gap-2">
                            <input
                              id={`${l.key}-q`}
                              inputMode="numeric"
                              value={l.quantity}
                              maxLength={3}
                              onChange={(e) => {
                                updateLine(i, { quantity: e.target.value });
                                touch(`medicines.${i}.quantity`);
                              }}
                              aria-invalid={!!qtyErr}
                              className={inputClass(!!qtyErr, "mt-0 w-16 text-right tabular-nums")}
                            />
                            <span className="truncate text-xs text-slate-500">{unit ?? ""}</span>
                          </div>
                        </div>
                        <button
                          type="button"
                          onClick={() => setLines((ls) => ls.filter((_, j) => j !== i))}
                          className={`${buttonClass("ghost", "sm")} mt-1`}
                          aria-label={`Remove medicine ${i + 1}`}
                        >
                          <Trash2 className="size-4" aria-hidden />
                        </button>
                      </div>
                      {(medErr || qtyErr) && (
                        <p className="mt-1 text-xs font-medium text-red-700">{medErr ?? qtyErr}</p>
                      )}
                    </li>
                  );
                })}
              </ul>
            )}
            {medicines?.length === 0 ? (
              <p className="mt-2 text-xs text-slate-500">No medicines are set up yet. An admin can add them in Settings.</p>
            ) : (
              <button
                type="button"
                className={`${buttonClass("secondary", "sm")} mt-2`}
                onClick={() => setLines((ls) => [...ls, newLine()])}
                disabled={lines.length >= MAX_LINES || !medicines}
              >
                <Plus className="size-3.5" aria-hidden /> Add medicine
              </button>
            )}
          </fieldset>
        </div>
      </Card>

      <div className="flex justify-end gap-2">
        <button type="button" className={buttonClass("secondary")} onClick={() => navigate(-1)} disabled={saving}>
          Cancel
        </button>
        <button type="submit" className={buttonClass("primary")} disabled={!valid || saving}>
          {saving && <Loader2 className="size-4 animate-spin" aria-hidden />}
          {editing ? "Save changes" : "Save visit"}
        </button>
      </div>
    </form>
  );
}

function FieldMsg({ id, error, hint }: { id: string; error: string | null; hint: string }) {
  return error ? (
    <p id={id} className="mt-1.5 text-xs font-medium text-red-700">
      {error}
    </p>
  ) : (
    <p id={id} className="mt-1.5 text-right text-xs text-slate-400 tabular-nums">
      {hint}
    </p>
  );
}
