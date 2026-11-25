import { CircleCheck, Copy, Loader2, Plus, Wand2 } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { Dialog } from "../../components/Dialog";
import { buttonClass, ErrorNotice, TextField } from "../../components/form";
import { asCmdError, superadminApi, type CreatedSchool, type NewSchoolInput, type SchoolSummary } from "../../lib/api";
import { formatDate, formatDateTime, planLabel } from "../../lib/format";

function statusOf(s: SchoolSummary): { label: string; className: string } {
  if (!s.status) return { label: "No plan", className: "text-slate-500" };
  if (s.is_trial) {
    return s.status === "expired"
      ? { label: "Trial ended", className: "text-red-700" }
      : { label: "Free trial", className: "text-sky-700" };
  }
  if (s.status === "expired") return { label: "Expired", className: "text-red-700" };
  if (s.status === "cancelled") return { label: "Cancelled", className: "text-slate-500" };
  return { label: "Active", className: "text-teal-800" };
}

/** Every school using Clinilog, where its subscription stands, and adding new ones. */
export function SchoolsPage() {
  const [schools, setSchools] = useState<SchoolSummary[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);
  const [created, setCreated] = useState<CreatedSchool | null>(null);

  const load = useCallback(() => {
    superadminApi
      .schools()
      .then((s) => {
        setSchools(s);
        setError(null);
      })
      .catch((e) => setError(asCmdError(e).message));
  }, []);

  useEffect(load, [load]);

  return (
    <div className="max-w-6xl space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <p className="text-sm text-slate-500">
          New schools start on a free 30-day Basic trial. After that they pay with GCash, or become view-only.
        </p>
        <button type="button" className={buttonClass("primary")} onClick={() => setAdding(true)}>
          <Plus className="size-4" aria-hidden /> Add school
        </button>
      </div>
      {error && <ErrorNotice>{error}</ErrorNotice>}
      <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        {!schools && !error && (
          <p className="flex items-center justify-center gap-2 py-12 text-sm text-slate-500">
            <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
          </p>
        )}
        {schools && schools.length === 0 && <p className="py-12 text-center text-sm text-slate-500">No schools yet.</p>}
        {schools && schools.length > 0 && (
          <table className="w-full text-left text-sm">
            <thead className="border-b border-slate-200 bg-slate-50 text-xs font-semibold tracking-wide text-slate-500 uppercase">
              <tr>
                <th scope="col" className="px-4 py-3">School</th>
                <th scope="col" className="px-4 py-3">Plan</th>
                <th scope="col" className="px-4 py-3">Status</th>
                <th scope="col" className="px-4 py-3">Valid until</th>
                <th scope="col" className="px-4 py-3 text-right">Accounts</th>
                <th scope="col" className="px-4 py-3 text-right">Students</th>
                <th scope="col" className="px-4 py-3">Last payment</th>
                <th scope="col" className="px-4 py-3">Waiting</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {schools.map((s) => {
                const st = statusOf(s);
                return (
                  <tr key={s.id}>
                    <td className="px-4 py-3">
                      <span className="font-medium">{s.name}</span>
                      <span className="block text-xs text-slate-500">
                        Code <span className="font-mono">{s.code}</span> · added {formatDate(s.created_at)}
                      </span>
                    </td>
                    <td className="px-4 py-3">{s.plan ? planLabel(s.plan) : "—"}</td>
                    <td className={`px-4 py-3 font-medium ${st.className}`}>{st.label}</td>
                    <td className="px-4 py-3 text-slate-600">{s.end_date ? formatDate(s.end_date) : "—"}</td>
                    <td className="px-4 py-3 text-right tabular-nums">{s.active_users}</td>
                    <td className="px-4 py-3 text-right tabular-nums">{s.students}</td>
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

      {adding && (
        <AddSchoolDialog
          onClose={() => setAdding(false)}
          onCreated={(c) => {
            setAdding(false);
            setCreated(c);
            load();
          }}
        />
      )}
      {created && <CreatedDialog created={created} onClose={() => setCreated(null)} />}
    </div>
  );
}

const EMPTY: NewSchoolInput = { name: "", code: "", address: "", full_name: "", username: "", password: "" };

/** A readable starting password: 4 letters, a hyphen, 4 digits (e.g. "Kpwa-4821"). */
function suggestPassword(): string {
  const letters = "abcdefghjkmnpqrstuvwxyz";
  const rand = (n: number) => crypto.getRandomValues(new Uint32Array(1))[0] % n;
  const word = Array.from({ length: 4 }, () => letters[rand(letters.length)]).join("");
  const digits = Array.from({ length: 4 }, () => String(rand(10))).join("");
  return `${word[0].toUpperCase()}${word.slice(1)}-${digits}`;
}

function AddSchoolDialog({ onClose, onCreated }: { onClose: () => void; onCreated: (c: CreatedSchool) => void }) {
  const [v, setV] = useState<NewSchoolInput>(EMPTY);
  const [fields, setFields] = useState<Record<string, string>>({});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const set = (k: keyof NewSchoolInput) => (value: string) => {
    setV((x) => ({ ...x, [k]: value }));
    setFields((f) => ({ ...f, [k]: "" }));
  };

  async function submit() {
    setBusy(true);
    setError(null);
    try {
      onCreated(await superadminApi.createSchool(v));
    } catch (e) {
      const err = asCmdError(e);
      if (err.fields) setFields(err.fields);
      else setError(err.message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog
      open
      wide
      onClose={() => !busy && onClose()}
      title="Add a school"
      footer={
        <>
          <button type="button" className={buttonClass("secondary")} onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="button" className={buttonClass("primary")} onClick={() => void submit()} disabled={busy}>
            {busy && <Loader2 className="size-4 animate-spin" aria-hidden />}
            Create school
          </button>
        </>
      }
    >
      <div className="grid gap-6 md:grid-cols-2">
        <fieldset className="space-y-4">
          <legend className="mb-2 text-sm font-semibold text-slate-800">School</legend>
          <TextField label="School name" value={v.name} onChange={(e) => set("name")(e.target.value)} error={fields.name} maxLength={200} autoFocus />
          <TextField
            label="School code"
            value={v.code}
            onChange={(e) => set("code")(e.target.value.toUpperCase())}
            error={fields.code}
            hint="3–20 letters, numbers or hyphens. Staff type it once when they install the app."
            maxLength={20}
            className="font-mono tracking-wider"
            placeholder="e.g. SINHS"
          />
          <TextField label="Address" optional value={v.address} onChange={(e) => set("address")(e.target.value)} error={fields.address} maxLength={300} />
        </fieldset>
        <fieldset className="space-y-4">
          <legend className="mb-2 text-sm font-semibold text-slate-800">First admin</legend>
          <TextField label="Full name" value={v.full_name} onChange={(e) => set("full_name")(e.target.value)} error={fields.full_name} maxLength={100} />
          <TextField
            label="Username"
            value={v.username}
            onChange={(e) => set("username")(e.target.value)}
            error={fields.username}
            hint="4–30 letters, numbers or underscore. Can be the same as in another school, e.g. admin."
            maxLength={30}
            autoComplete="off"
          />
          <div>
            <TextField
              label="Starting password"
              value={v.password}
              onChange={(e) => set("password")(e.target.value)}
              error={fields.password}
              hint="At least 8 characters with a letter and a number. The admin can change it in User Accounts."
              maxLength={128}
              autoComplete="off"
              spellCheck={false}
              className="font-mono"
            />
            <button type="button" className={`${buttonClass("ghost", "sm")} mt-1`} onClick={() => set("password")(suggestPassword())}>
              <Wand2 className="size-3.5" aria-hidden /> Suggest a password
            </button>
          </div>
        </fieldset>
      </div>
      <p className="mt-4 rounded-lg bg-sky-50 px-3 py-2 text-xs text-sky-900 ring-1 ring-sky-200">
        The school starts on a <strong>free 30-day Basic trial</strong> today, with the usual medicines already listed. The admin adds
        sections and staff accounts.
      </p>
      {error && (
        <div className="mt-3">
          <ErrorNotice>{error}</ErrorNotice>
        </div>
      )}
    </Dialog>
  );
}

function CreatedDialog({ created, onClose }: { created: CreatedSchool; onClose: () => void }) {
  const [copied, setCopied] = useState(false);
  const text = [
    `Clinilog is ready for ${created.school.name}.`,
    `School code: ${created.school.code}`,
    `Admin username: ${created.admin_username}`,
    "Password: (the starting password you set)",
    `Free trial until ${formatDate(created.trial_end)}.`,
  ].join("\n");

  return (
    <Dialog
      open
      onClose={onClose}
      title="School created"
      footer={
        <button type="button" className={buttonClass("primary")} onClick={onClose} autoFocus>
          Done
        </button>
      }
    >
      <div className="space-y-3">
        <p className="flex items-center gap-2 font-medium text-teal-800">
          <CircleCheck className="size-5" aria-hidden /> {created.school.name} is on a free trial.
        </p>
        <dl className="grid grid-cols-2 gap-x-4 gap-y-2 rounded-lg bg-slate-50 p-3 text-sm">
          <dt className="text-slate-500">School code</dt>
          <dd className="font-mono font-semibold">{created.school.code}</dd>
          <dt className="text-slate-500">Admin</dt>
          <dd>
            {created.admin_full_name} <span className="font-mono text-slate-600">({created.admin_username})</span>
          </dd>
          <dt className="text-slate-500">Trial</dt>
          <dd>
            {formatDate(created.trial_start)} – {formatDate(created.trial_end)}
          </dd>
        </dl>
        <p>
          Send the admin the Clinilog installer, the school code, their username and the starting password. On first launch the app asks
          for the school code.
        </p>
        <button
          type="button"
          className={buttonClass("secondary", "sm")}
          onClick={() => {
            void navigator.clipboard.writeText(text).then(() => setCopied(true));
          }}
        >
          <Copy className="size-3.5" aria-hidden /> {copied ? "Copied" : "Copy details (without password)"}
        </button>
      </div>
    </Dialog>
  );
}
