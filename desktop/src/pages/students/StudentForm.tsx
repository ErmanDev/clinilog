import { Loader2 } from "lucide-react";
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { ConfirmDialog } from "../../components/Dialog";
import { buttonClass, ErrorNotice, SelectField, TextField } from "../../components/form";
import { Card } from "../../components/ui";
import { asCmdError, studentsApi, type DuplicateMatch, type StudentInput } from "../../lib/api";
import { sectionLabel } from "../../lib/format";
import { usePlan } from "../../lib/session";
import { groupByGrade, useSections } from "../../lib/useSections";
import {
  birthdateBounds,
  validateBirthdate,
  validateGender,
  validateLrn,
  validateName,
} from "../../lib/validation";

type FieldName = "lrn" | "first_name" | "middle_name" | "last_name" | "gender" | "birthdate" | "section_id";

const EMPTY: StudentInput = {
  lrn: "",
  first_name: "",
  middle_name: "",
  last_name: "",
  gender: "",
  birthdate: "",
  section_id: null,
};

function clientErrors(v: StudentInput): Partial<Record<FieldName, string>> {
  const e: Partial<Record<FieldName, string>> = {};
  const set = (k: FieldName, msg: string | null) => msg && (e[k] = msg);
  set("lrn", validateLrn(v.lrn));
  set("first_name", validateName(v.first_name, "first name"));
  set("middle_name", validateName(v.middle_name, "middle name", false));
  set("last_name", validateName(v.last_name, "last name"));
  set("gender", validateGender(v.gender));
  set("birthdate", validateBirthdate(v.birthdate));
  set("section_id", v.section_id ? null : "Choose a section.");
  return e;
}

/** `/students/new` and `/students/:id/edit` */
export function StudentFormPage() {
  const { id } = useParams();
  const editing = !!id;
  const navigate = useNavigate();
  const { readOnly } = usePlan();
  const { sections, error: sectionsError } = useSections();

  // A client-made id makes "save" safe to retry if the connection drops mid-request.
  const newId = useMemo(() => crypto.randomUUID(), []);
  const [values, setValues] = useState<StudentInput>(EMPTY);
  const [loaded, setLoaded] = useState(!editing);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [touched, setTouched] = useState<Partial<Record<FieldName, boolean>>>({});
  const [serverErrors, setServerErrors] = useState<Partial<Record<FieldName, string>>>({});
  const [formError, setFormError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [duplicates, setDuplicates] = useState<DuplicateMatch[] | null>(null);

  useEffect(() => {
    if (!id) return;
    studentsApi
      .get(id)
      .then((s) => {
        setValues({
          lrn: s.lrn,
          first_name: s.first_name,
          middle_name: s.middle_name ?? "",
          last_name: s.last_name,
          gender: s.gender,
          birthdate: s.birthdate,
          section_id: s.section.id,
        });
        setLoaded(true);
      })
      .catch((e) => setLoadError(asCmdError(e).message));
  }, [id]);

  // Live "already registered?" check once the LRN is 12 digits. Save re-checks on the
  // server anyway, so a failed lookup is ignored rather than blocking the form.
  const lrnTrimmed = values.lrn.trim();
  const lrnFormatOk = validateLrn(values.lrn) === null;
  const [lrnCheck, setLrnCheck] = useState<
    { lrn: string; state: "checking" } | { lrn: string; state: "available" } | { lrn: string; state: "taken"; owner: { id: string; first_name: string; last_name: string } } | null
  >(null);
  useEffect(() => {
    if (!lrnFormatOk) {
      setLrnCheck(null);
      return;
    }
    let cancelled = false;
    setLrnCheck({ lrn: lrnTrimmed, state: "checking" });
    const t = window.setTimeout(() => {
      studentsApi
        .lrnCheck(lrnTrimmed, id)
        .then((r) => {
          if (cancelled) return;
          setLrnCheck(r.available || !r.student ? { lrn: lrnTrimmed, state: "available" } : { lrn: lrnTrimmed, state: "taken", owner: r.student });
        })
        .catch(() => !cancelled && setLrnCheck(null));
    }, 350);
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [lrnTrimmed, lrnFormatOk, id]);
  const lrnTaken = lrnCheck?.state === "taken" && lrnCheck.lrn === lrnTrimmed ? lrnCheck.owner : null;

  const errors = clientErrors(values);
  const valid = Object.keys(errors).length === 0 && !lrnTaken;
  const bounds = birthdateBounds();

  function update<K extends keyof StudentInput>(key: K, value: StudentInput[K]) {
    setValues((v) => ({ ...v, [key]: value }));
    // A server message is about the old value; drop it once the field changes.
    setServerErrors((e) => ({ ...e, [key]: undefined }));
    setTouched((t) => ({ ...t, [key]: true }));
  }

  // Show the instant check once a field has been touched; server messages always.
  const shown = (k: FieldName) => serverErrors[k] ?? (touched[k] ? errors[k] : undefined) ?? null;
  const touch = (k: FieldName) => () => setTouched((t) => ({ ...t, [k]: true }));

  async function save(allowDuplicate: boolean) {
    setSaving(true);
    setFormError(null);
    const body: StudentInput = { ...values, allow_duplicate: allowDuplicate, ...(editing ? {} : { id: newId }) };
    try {
      const saved = editing ? await studentsApi.update(id!, body) : await studentsApi.create(body);
      navigate(`/students/${saved.id}`, { replace: true });
    } catch (e) {
      const err = asCmdError(e);
      if (err.kind === "conflict" && err.code === "possible_duplicate") {
        const details = err.details as { matches?: DuplicateMatch[] } | undefined;
        setDuplicates(details?.matches ?? []);
      } else if (err.kind === "invalid" && err.fields) {
        setServerErrors(err.fields as Partial<Record<FieldName, string>>);
        setFormError(err.message);
      } else {
        setFormError(err.message);
      }
    } finally {
      setSaving(false);
    }
  }

  function onSubmit(e: FormEvent) {
    e.preventDefault();
    if (!valid || saving) return;
    void save(false);
  }

  if (loadError) return <ErrorNotice>{loadError}</ErrorNotice>;
  if (!loaded) {
    return (
      <div className="flex items-center gap-2 text-sm text-slate-500">
        <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
      </div>
    );
  }
  if (readOnly) {
    return (
      <ErrorNotice>
        Records can't be added or changed while the subscription is inactive.{" "}
        <Link to="/students" className="font-semibold underline">
          Back to students
        </Link>
      </ErrorNotice>
    );
  }

  const lrnDigits = values.lrn.trim().length;

  return (
    <form onSubmit={onSubmit} noValidate className="max-w-3xl space-y-4">
      <h2 className="text-base font-semibold">{editing ? "Edit student" : "Add student"}</h2>

      {formError && <ErrorNotice>{formError}</ErrorNotice>}
      {sectionsError && <ErrorNotice>Couldn't load sections: {sectionsError}</ErrorNotice>}

      <Card>
        <div className="grid gap-5 md:grid-cols-2">
          <TextField
            label="LRN"
            value={values.lrn}
            onChange={(e) => update("lrn", e.target.value)}
            onBlur={touch("lrn")}
            error={
              lrnTaken ? (
                <span>
                  Already registered to{" "}
                  <Link to={`/students/${lrnTaken.id}`} className="underline">
                    {lrnTaken.last_name}, {lrnTaken.first_name}
                  </Link>
                  .
                </span>
              ) : (
                shown("lrn")
              )
            }
            hint={
              lrnCheck?.state === "checking"
                ? `${lrnDigits}/12 digits · checking…`
                : lrnCheck?.state === "available" && lrnCheck.lrn === lrnTrimmed
                  ? "12/12 digits · not yet registered in your school"
                  : `${lrnDigits}/12 digits`
            }
            inputMode="numeric"
            autoComplete="off"
            maxLength={12}
            autoFocus={!editing}
            className="font-mono tabular-nums"
          />
          <div className="hidden md:block" />
          <TextField
            label="First name"
            value={values.first_name}
            onChange={(e) => update("first_name", e.target.value)}
            onBlur={touch("first_name")}
            error={shown("first_name")}
            maxLength={50}
            autoComplete="off"
          />
          <TextField
            label="Middle name"
            optional
            value={values.middle_name}
            onChange={(e) => update("middle_name", e.target.value)}
            onBlur={touch("middle_name")}
            error={shown("middle_name")}
            maxLength={50}
            autoComplete="off"
          />
          <TextField
            label="Last name"
            value={values.last_name}
            onChange={(e) => update("last_name", e.target.value)}
            onBlur={touch("last_name")}
            error={shown("last_name")}
            maxLength={50}
            autoComplete="off"
          />
          <SelectField
            label="Gender"
            value={values.gender}
            onChange={(e) => update("gender", e.target.value as StudentInput["gender"])}
            onBlur={touch("gender")}
            error={shown("gender")}
          >
            <option value="" disabled>
              Select…
            </option>
            <option value="female">Female</option>
            <option value="male">Male</option>
          </SelectField>
          <TextField
            label="Birthdate"
            type="date"
            value={values.birthdate}
            min={bounds.min}
            max={bounds.max}
            onChange={(e) => update("birthdate", e.target.value)}
            onBlur={touch("birthdate")}
            error={shown("birthdate")}
            hint="Age must be 4 to 25."
          />
          <SelectField
            label="Grade & section"
            value={values.section_id ?? ""}
            onChange={(e) => update("section_id", e.target.value || null)}
            onBlur={touch("section_id")}
            error={shown("section_id")}
            disabled={!sections}
          >
            <option value="" disabled>
              {sections ? (sections.length ? "Select…" : "No sections yet — ask your admin") : "Loading…"}
            </option>
            {sections &&
              groupByGrade(sections).map(([grade, list]) => (
                <optgroup key={grade} label={`Grade ${grade}`}>
                  {list.map((s) => (
                    <option key={s.id} value={s.id}>
                      {sectionLabel(s)}
                    </option>
                  ))}
                </optgroup>
              ))}
          </SelectField>
        </div>
      </Card>

      <div className="flex justify-end gap-2">
        <button type="button" className={buttonClass("secondary")} onClick={() => navigate(-1)} disabled={saving}>
          Cancel
        </button>
        <button type="submit" className={buttonClass("primary")} disabled={!valid || saving}>
          {saving && <Loader2 className="size-4 animate-spin" aria-hidden />}
          {editing ? "Save changes" : "Save student"}
        </button>
      </div>

      <ConfirmDialog
        open={duplicates !== null}
        title="Possible duplicate"
        confirmLabel="Save anyway"
        tone="primary"
        busy={saving}
        onCancel={() => setDuplicates(null)}
        onConfirm={() => {
          setDuplicates(null);
          void save(true);
        }}
      >
        <p>A student with the same name and birthdate is already registered:</p>
        <ul className="space-y-1 rounded-lg bg-slate-50 p-3">
          {duplicates?.map((m) => (
            <li key={m.id}>
              <Link to={`/students/${m.id}`} className="font-medium text-teal-800 hover:underline">
                {m.last_name}, {m.first_name}
              </Link>{" "}
              <span className="text-slate-500">
                · LRN <span className="font-mono">{m.lrn}</span> · {sectionLabel(m)}
              </span>
            </li>
          ))}
        </ul>
        <p>If this is a different student, you can save anyway.</p>
      </ConfirmDialog>
    </form>
  );
}
