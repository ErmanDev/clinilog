import { CircleCheck, CircleSlash, Eye, EyeOff, KeyRound, Loader2, Pencil, Plus, ShieldCheck, UserRound } from "lucide-react";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import { ConfirmDialog, Dialog } from "../components/Dialog";
import { buttonClass, ErrorNotice, Field, inputClass, SelectField, TextField } from "../components/form";
import { asCmdError, usersApi, type Account, type AccountRole } from "../lib/api";
import { formatDate } from "../lib/format";
import { usePlan, useSession } from "../lib/session";
import { validateFullName, validatePassword, validateUsername } from "../lib/validation";

type Modal =
  | { kind: "add" }
  | { kind: "edit"; user: Account }
  | { kind: "reset"; user: Account }
  | { kind: "toggle"; user: Account }
  | null;

export function UserAccountsPage() {
  const { session, refreshPlan } = useSession();
  const { info, readOnly } = usePlan();
  const [users, setUsers] = useState<Account[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [modal, setModal] = useState<Modal>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setUsers(await usersApi.list());
      setError(null);
    } catch (e) {
      setError(asCmdError(e).message);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  async function done(message: string) {
    setModal(null);
    setNotice(message);
    await Promise.all([reload(), refreshPlan()]);
  }

  const limit = info?.limits.max_users ?? null;
  const active = users?.filter((u) => u.is_active).length ?? 0;
  const atLimit = limit !== null && active >= limit;
  const addBlocked = readOnly
    ? "Adding accounts is paused while the subscription is inactive."
    : atLimit
      ? `Your plan allows ${limit} active accounts. Disable an unused account${limit < 10 ? " or upgrade to Premium" : ""} to add another.`
      : null;

  return (
    <div className="max-w-5xl space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="min-w-64">
          <p className="text-sm text-slate-600">
            <span className="font-semibold text-slate-900 tabular-nums">{active}</span> of{" "}
            <span className="tabular-nums">{limit ?? "—"}</span> active accounts used
          </p>
          {limit !== null && (
            <div className="mt-1.5 h-2 w-64 overflow-hidden rounded-full bg-slate-200" aria-hidden>
              <div
                className={`h-full rounded-full ${atLimit ? "bg-amber-500" : "bg-teal-600"}`}
                style={{ width: `${Math.min(100, (active / limit) * 100)}%` }}
              />
            </div>
          )}
        </div>
        <button
          type="button"
          className={buttonClass("primary")}
          disabled={!!addBlocked || !users}
          title={addBlocked ?? undefined}
          onClick={() => setModal({ kind: "add" })}
        >
          <Plus className="size-4" aria-hidden /> Add user
        </button>
      </div>
      {addBlocked && atLimit && <p className="text-sm text-amber-800">{addBlocked}</p>}
      {notice && (
        <p role="status" className="rounded-lg bg-teal-50 px-4 py-3 text-sm text-teal-900 ring-1 ring-teal-200">
          {notice}
        </p>
      )}
      {error && <ErrorNotice>{error}</ErrorNotice>}

      <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        {!users && !error && (
          <p className="flex items-center justify-center gap-2 py-12 text-sm text-slate-500">
            <Loader2 className="size-4 animate-spin" aria-hidden /> Loading accounts…
          </p>
        )}
        {users && (
          <table className="w-full text-left text-sm">
            <thead className="border-b border-slate-200 bg-slate-50 text-xs font-semibold tracking-wide text-slate-500 uppercase">
              <tr>
                <th scope="col" className="px-4 py-3">Name</th>
                <th scope="col" className="px-4 py-3">Username</th>
                <th scope="col" className="px-4 py-3">Role</th>
                <th scope="col" className="px-4 py-3">Status</th>
                <th scope="col" className="px-4 py-3">Added</th>
                <th scope="col" className="px-4 py-3 text-right">
                  <span className="sr-only">Actions</span>
                </th>
              </tr>
            </thead>
            <tbody className="divide-y divide-slate-100">
              {users.map((u) => {
                const self = u.id === session?.user.id;
                return (
                  <tr key={u.id} className={u.is_active ? "" : "bg-slate-50/60 text-slate-500"}>
                    <td className="px-4 py-3 font-medium">
                      {u.full_name}
                      {self && <span className="ml-2 rounded bg-slate-100 px-1.5 py-0.5 text-xs font-medium text-slate-600">You</span>}
                    </td>
                    <td className="px-4 py-3 font-mono text-slate-600">{u.username}</td>
                    <td className="px-4 py-3">
                      <span className="inline-flex items-center gap-1.5">
                        {u.role === "admin" ? <ShieldCheck className="size-4 text-teal-700" aria-hidden /> : <UserRound className="size-4 text-slate-400" aria-hidden />}
                        {u.role === "admin" ? "Admin" : "Staff"}
                      </span>
                    </td>
                    <td className="px-4 py-3">
                      <span className="inline-flex items-center gap-1.5">
                        {u.is_active ? <CircleCheck className="size-4 text-teal-700" aria-hidden /> : <CircleSlash className="size-4 text-slate-400" aria-hidden />}
                        {u.is_active ? "Active" : "Disabled"}
                      </span>
                    </td>
                    <td className="px-4 py-3 text-slate-600">{formatDate(u.created_at)}</td>
                    <td className="px-4 py-2 text-right whitespace-nowrap">
                      <button type="button" className={buttonClass("ghost", "sm")} disabled={readOnly} onClick={() => setModal({ kind: "edit", user: u })}>
                        <Pencil className="size-3.5" aria-hidden /> Edit
                      </button>
                      <button type="button" className={buttonClass("ghost", "sm")} onClick={() => setModal({ kind: "reset", user: u })}>
                        <KeyRound className="size-3.5" aria-hidden /> Reset password
                      </button>
                      {!self && (
                        <button
                          type="button"
                          className={buttonClass("ghost", "sm")}
                          disabled={!u.is_active && (readOnly || atLimit)}
                          title={!u.is_active && atLimit ? `Your plan allows ${limit} active accounts.` : undefined}
                          onClick={() => setModal({ kind: "toggle", user: u })}
                        >
                          {u.is_active ? <CircleSlash className="size-3.5" aria-hidden /> : <CircleCheck className="size-3.5" aria-hidden />}
                          {u.is_active ? "Disable" : "Enable"}
                        </button>
                      )}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        )}
      </div>

      {modal?.kind === "add" && (
        <AddUserDialog onClose={() => setModal(null)} onDone={(u) => void done(`Added ${u.full_name}. They can now log in as “${u.username}”.`)} />
      )}
      {modal?.kind === "edit" && (
        <EditUserDialog user={modal.user} self={modal.user.id === session?.user.id} onClose={() => setModal(null)} onDone={(u) => void done(`Saved changes to ${u.full_name}.`)} />
      )}
      {modal?.kind === "reset" && (
        <ResetPasswordDialog user={modal.user} onClose={() => setModal(null)} onDone={() => void done(`New password set for ${modal.user.full_name}. Give it to them in person.`)} />
      )}
      {modal?.kind === "toggle" && (
        <ToggleDialog user={modal.user} onClose={() => setModal(null)} onDone={(u) => void done(`${u.full_name} is now ${u.is_active ? "active" : "disabled"}.`)} />
      )}
    </div>
  );
}

// ---------- dialogs ----------

function PasswordField({
  label,
  value,
  onChange,
  error,
  hint,
  autoFocus,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  error?: string | null;
  hint?: string;
  autoFocus?: boolean;
}) {
  const [show, setShow] = useState(false);
  return (
    <Field label={label} error={error} hint={hint}>
      {({ id, describedBy, invalid }) => (
        <div className="relative">
          <input
            id={id}
            type={show ? "text" : "password"}
            value={value}
            autoComplete="new-password"
            autoFocus={autoFocus}
            maxLength={128}
            onChange={(e) => onChange(e.target.value)}
            aria-invalid={invalid}
            aria-describedby={describedBy}
            className={inputClass(invalid, "pr-10")}
          />
          <button
            type="button"
            onClick={() => setShow((s) => !s)}
            className="absolute inset-y-0 right-0 mt-1 flex items-center px-3 text-slate-500 hover:text-slate-700"
            aria-label={show ? "Hide password" : "Show password"}
          >
            {show ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
          </button>
        </div>
      )}
    </Field>
  );
}

const PW_HINT = "At least 8 characters, with a letter and a number. Passwords are case-sensitive.";

function useFormState<T extends Record<string, string>>(initial: T) {
  const [values, setValues] = useState(initial);
  const [touched, setTouched] = useState<Partial<Record<keyof T, boolean>>>({});
  const [serverErrors, setServerErrors] = useState<Record<string, string>>({});
  const set = (k: keyof T) => (v: string) => {
    setValues((x) => ({ ...x, [k]: v }));
    setTouched((t) => ({ ...t, [k]: true }));
    setServerErrors((e) => ({ ...e, [k as string]: "" }));
  };
  return { values, touched, serverErrors, setServerErrors, set };
}

function AddUserDialog({ onClose, onDone }: { onClose: () => void; onDone: (u: Account) => void }) {
  const f = useFormState({ full_name: "", username: "", role: "staff", password: "", confirm: "" });
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);

  const errors: Record<string, string | null> = {
    full_name: validateFullName(f.values.full_name),
    username: validateUsername(f.values.username),
    password: validatePassword(f.values.password),
    confirm: f.values.confirm !== f.values.password ? "Passwords don't match." : null,
  };
  const valid = Object.values(errors).every((e) => !e);
  const shown = (k: string) => f.serverErrors[k] || (f.touched[k as keyof typeof f.values] ? errors[k] : null);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!valid) return;
    setSaving(true);
    setFormError(null);
    try {
      const u = await usersApi.create({
        full_name: f.values.full_name,
        username: f.values.username,
        password: f.values.password,
        role: f.values.role as AccountRole,
      });
      onDone(u);
    } catch (err) {
      const ce = asCmdError(err);
      if (ce.fields) f.setServerErrors(ce.fields);
      setFormError(ce.message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open onClose={() => !saving && onClose()} title="Add user">
      <form onSubmit={submit} noValidate className="space-y-4">
          {formError && <ErrorNotice>{formError}</ErrorNotice>}
          <TextField label="Full name" value={f.values.full_name} onChange={(e) => f.set("full_name")(e.target.value)} error={shown("full_name")} maxLength={100} autoFocus />
          <TextField
            label="Username"
            value={f.values.username}
            onChange={(e) => f.set("username")(e.target.value)}
            error={shown("username")}
            hint="4–30 letters, numbers or underscore. Not case-sensitive when logging in."
            maxLength={30}
            autoComplete="off"
          />
          <SelectField label="Role" value={f.values.role} onChange={(e) => f.set("role")(e.target.value)} hint="Admins can also manage accounts, sections, medicines and settings.">
            <option value="staff">Staff</option>
            <option value="admin">Admin</option>
          </SelectField>
          <PasswordField label="Password" value={f.values.password} onChange={f.set("password")} error={shown("password")} hint={PW_HINT} />
          <PasswordField label="Confirm password" value={f.values.confirm} onChange={f.set("confirm")} error={shown("confirm")} />
          <div className="flex justify-end gap-2 pt-2">
            <button type="button" className={buttonClass("secondary")} onClick={onClose} disabled={saving}>
              Cancel
            </button>
            <button type="submit" className={buttonClass("primary")} disabled={!valid || saving}>
              {saving && <Loader2 className="size-4 animate-spin" aria-hidden />} Add user
            </button>
          </div>
        </form>
    </Dialog>
  );
}

function EditUserDialog({ user, self, onClose, onDone }: { user: Account; self: boolean; onClose: () => void; onDone: (u: Account) => void }) {
  const f = useFormState({ full_name: user.full_name, role: user.role as string });
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const nameError = validateFullName(f.values.full_name);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (nameError) return;
    setSaving(true);
    setFormError(null);
    try {
      onDone(await usersApi.update(user.id, { full_name: f.values.full_name, role: f.values.role as AccountRole }));
    } catch (err) {
      const ce = asCmdError(err);
      if (ce.fields) f.setServerErrors(ce.fields);
      setFormError(ce.message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open onClose={() => !saving && onClose()} title={`Edit ${user.username}`}>
      <form onSubmit={submit} noValidate className="space-y-4">
        {formError && <ErrorNotice>{formError}</ErrorNotice>}
        <TextField
          label="Full name"
          value={f.values.full_name}
          onChange={(e) => f.set("full_name")(e.target.value)}
          error={f.serverErrors.full_name || (f.touched.full_name ? nameError : null)}
          maxLength={100}
          autoFocus
        />
        <SelectField
          label="Role"
          value={f.values.role}
          onChange={(e) => f.set("role")(e.target.value)}
          disabled={self}
          hint={self ? "You can't change your own role." : undefined}
        >
          <option value="staff">Staff</option>
          <option value="admin">Admin</option>
        </SelectField>
        <div className="flex justify-end gap-2 pt-2">
          <button type="button" className={buttonClass("secondary")} onClick={onClose} disabled={saving}>
            Cancel
          </button>
          <button type="submit" className={buttonClass("primary")} disabled={!!nameError || saving}>
            {saving && <Loader2 className="size-4 animate-spin" aria-hidden />} Save
          </button>
        </div>
      </form>
    </Dialog>
  );
}

function ResetPasswordDialog({ user, onClose, onDone }: { user: Account; onClose: () => void; onDone: () => void }) {
  const f = useFormState({ password: "", confirm: "" });
  const [saving, setSaving] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const pwError = validatePassword(f.values.password);
  const confirmError = f.values.confirm !== f.values.password ? "Passwords don't match." : null;

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (pwError || confirmError) return;
    setSaving(true);
    setFormError(null);
    try {
      await usersApi.resetPassword(user.id, f.values.password);
      onDone();
    } catch (err) {
      const ce = asCmdError(err);
      setFormError(ce.fields?.password ?? ce.message);
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog open onClose={() => !saving && onClose()} title="Reset password">
      <form onSubmit={submit} noValidate className="space-y-4">
        <p>
          Set a new password for <strong className="font-semibold">{user.full_name}</strong> ({user.username}). This also clears a login
          lock after too many failed attempts.
        </p>
        {formError && <ErrorNotice>{formError}</ErrorNotice>}
        <PasswordField label="New password" value={f.values.password} onChange={f.set("password")} error={f.touched.password ? pwError : null} hint={PW_HINT} autoFocus />
        <PasswordField label="Confirm new password" value={f.values.confirm} onChange={f.set("confirm")} error={f.touched.confirm ? confirmError : null} />
        <div className="flex justify-end gap-2 pt-2">
          <button type="button" className={buttonClass("secondary")} onClick={onClose} disabled={saving}>
            Cancel
          </button>
          <button type="submit" className={buttonClass("primary")} disabled={!!pwError || !!confirmError || saving}>
            {saving && <Loader2 className="size-4 animate-spin" aria-hidden />} Set password
          </button>
        </div>
      </form>
    </Dialog>
  );
}

function ToggleDialog({ user, onClose, onDone }: { user: Account; onClose: () => void; onDone: (u: Account) => void }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const disabling = user.is_active;

  async function go() {
    setBusy(true);
    setError(null);
    try {
      onDone(disabling ? await usersApi.disable(user.id) : await usersApi.enable(user.id));
    } catch (e) {
      setError(asCmdError(e).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <ConfirmDialog
      open
      title={disabling ? "Disable account?" : "Enable account?"}
      confirmLabel={disabling ? "Disable account" : "Enable account"}
      tone={disabling ? "danger" : "primary"}
      busy={busy}
      error={error}
      onCancel={onClose}
      onConfirm={() => void go()}
    >
      <p>
        {disabling ? (
          <>
            <strong className="font-semibold">{user.full_name}</strong> will be logged out and won't be able to log in. Their past visits and
            records stay. You can enable the account again later.
          </>
        ) : (
          <>
            <strong className="font-semibold">{user.full_name}</strong> will be able to log in again. This uses one of your plan's active
            accounts.
          </>
        )}
      </p>
    </ConfirmDialog>
  );
}
