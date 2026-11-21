import { Check, Loader2, Pencil, Plus, Trash2, X } from "lucide-react";
import { useCallback, useEffect, useState, type FormEvent } from "react";
import { ConfirmDialog } from "../components/Dialog";
import { buttonClass, ErrorNotice, inputClass, TextField } from "../components/form";
import { Card } from "../components/ui";
import { asCmdError, medicinesApi, type Medicine } from "../lib/api";
import { usePlan } from "../lib/session";
import { validateShortText } from "../lib/validation";

/** Admin list of medicines the clinic gives out (Settings page). */
export function MedicinesManager() {
  const { readOnly } = usePlan();
  const [items, setItems] = useState<Medicine[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  const [name, setName] = useState("");
  const [unit, setUnit] = useState("");
  const [addErrors, setAddErrors] = useState<{ name?: string; unit?: string; form?: string }>({});
  const [adding, setAdding] = useState(false);

  const [editing, setEditing] = useState<{ id: string; name: string; unit: string } | null>(null);
  const [editError, setEditError] = useState<string | null>(null);
  const [savingEdit, setSavingEdit] = useState(false);

  const [toDelete, setToDelete] = useState<Medicine | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const reload = useCallback(async () => {
    try {
      setItems(await medicinesApi.list());
      setLoadError(null);
    } catch (e) {
      setLoadError(asCmdError(e).message);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const addValid = !validateShortText(name, "medicine name", 100) && !validateShortText(unit, "unit", 30);

  async function onAdd(e: FormEvent) {
    e.preventDefault();
    if (!addValid) return;
    setAdding(true);
    setAddErrors({});
    try {
      await medicinesApi.create({ name, unit });
      setName("");
      setUnit("");
      await reload();
    } catch (err) {
      const ce = asCmdError(err);
      setAddErrors(ce.fields ? { name: ce.fields.name, unit: ce.fields.unit } : { form: ce.message });
    } finally {
      setAdding(false);
    }
  }

  async function onSaveEdit() {
    if (!editing) return;
    const msg = validateShortText(editing.name, "medicine name", 100) ?? validateShortText(editing.unit, "unit", 30);
    if (msg) return setEditError(msg);
    setSavingEdit(true);
    setEditError(null);
    try {
      await medicinesApi.update(editing.id, { name: editing.name, unit: editing.unit });
      setEditing(null);
      await reload();
    } catch (err) {
      const ce = asCmdError(err);
      setEditError(ce.fields?.name ?? ce.fields?.unit ?? ce.message);
    } finally {
      setSavingEdit(false);
    }
  }

  async function onDelete() {
    if (!toDelete) return;
    setDeleting(true);
    setDeleteError(null);
    try {
      await medicinesApi.remove(toDelete.id);
      setToDelete(null);
      await reload();
    } catch (err) {
      setDeleteError(asCmdError(err).message);
    } finally {
      setDeleting(false);
    }
  }

  return (
    <Card title="Medicines">
      <p className="-mt-2 mb-4 text-sm text-slate-500">Staff pick from this list when recording a visit.</p>

      {!readOnly && (
        <form onSubmit={onAdd} noValidate className="mb-5 flex flex-wrap items-start gap-3 rounded-lg bg-slate-50 p-4">
          <div className="min-w-56 flex-1">
            <TextField
              label="Medicine name"
              value={name}
              maxLength={100}
              placeholder="e.g. Paracetamol 500 mg"
              onChange={(e) => {
                setName(e.target.value);
                setAddErrors((x) => ({ ...x, name: undefined }));
              }}
              error={addErrors.name}
            />
          </div>
          <div className="w-36">
            <TextField
              label="Unit"
              value={unit}
              maxLength={30}
              placeholder="tablet"
              onChange={(e) => {
                setUnit(e.target.value);
                setAddErrors((x) => ({ ...x, unit: undefined }));
              }}
              error={addErrors.unit}
            />
          </div>
          <button type="submit" className={`${buttonClass("primary")} mt-6`} disabled={!addValid || adding}>
            {adding ? <Loader2 className="size-4 animate-spin" aria-hidden /> : <Plus className="size-4" aria-hidden />}
            Add medicine
          </button>
          {addErrors.form && (
            <div className="w-full">
              <ErrorNotice>{addErrors.form}</ErrorNotice>
            </div>
          )}
        </form>
      )}

      {loadError && <ErrorNotice>{loadError}</ErrorNotice>}
      {!items && !loadError && (
        <p className="flex items-center gap-2 text-sm text-slate-500">
          <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
        </p>
      )}
      {items?.length === 0 && <p className="text-sm text-slate-500">No medicines yet.</p>}

      {items && items.length > 0 && (
        <ul className="divide-y divide-slate-100 rounded-lg border border-slate-200">
          {items.map((m) => (
            <li key={m.id} className="flex items-center gap-3 px-3 py-2">
              {editing?.id === m.id ? (
                <div className="flex-1">
                  <div className="flex gap-2">
                    <label className="sr-only" htmlFor={`med-name-${m.id}`}>Medicine name</label>
                    <input
                      id={`med-name-${m.id}`}
                      value={editing.name}
                      maxLength={100}
                      autoFocus
                      onChange={(e) => setEditing({ ...editing, name: e.target.value })}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void onSaveEdit();
                        if (e.key === "Escape") setEditing(null);
                      }}
                      className={inputClass(!!editError, "mt-0 flex-1 py-1.5")}
                    />
                    <label className="sr-only" htmlFor={`med-unit-${m.id}`}>Unit</label>
                    <input
                      id={`med-unit-${m.id}`}
                      value={editing.unit}
                      maxLength={30}
                      onChange={(e) => setEditing({ ...editing, unit: e.target.value })}
                      onKeyDown={(e) => {
                        if (e.key === "Enter") void onSaveEdit();
                        if (e.key === "Escape") setEditing(null);
                      }}
                      className={inputClass(!!editError, "mt-0 w-28 py-1.5")}
                    />
                  </div>
                  {editError && <p className="mt-1 text-xs font-medium text-red-700">{editError}</p>}
                </div>
              ) : (
                <div className="flex-1">
                  <span className="text-sm font-medium">{m.name}</span>
                  <span className="ml-2 text-xs text-slate-500">per {m.unit}</span>
                </div>
              )}
              <span className="text-xs whitespace-nowrap text-slate-500 tabular-nums">
                {m.visit_count} visit{m.visit_count === 1 ? "" : "s"}
              </span>
              {!readOnly &&
                (editing?.id === m.id ? (
                  <>
                    <button type="button" className={buttonClass("ghost", "sm")} onClick={() => void onSaveEdit()} disabled={savingEdit} aria-label="Save">
                      {savingEdit ? <Loader2 className="size-4 animate-spin" /> : <Check className="size-4" />}
                    </button>
                    <button type="button" className={buttonClass("ghost", "sm")} onClick={() => setEditing(null)} aria-label="Cancel">
                      <X className="size-4" />
                    </button>
                  </>
                ) : (
                  <>
                    <button
                      type="button"
                      className={buttonClass("ghost", "sm")}
                      onClick={() => {
                        setEditing({ id: m.id, name: m.name, unit: m.unit });
                        setEditError(null);
                      }}
                      aria-label={`Edit ${m.name}`}
                    >
                      <Pencil className="size-4" />
                    </button>
                    <button
                      type="button"
                      className={buttonClass("ghost", "sm")}
                      onClick={() => {
                        setDeleteError(null);
                        setToDelete(m);
                      }}
                      aria-label={`Delete ${m.name}`}
                    >
                      <Trash2 className="size-4 text-red-600" />
                    </button>
                  </>
                ))}
            </li>
          ))}
        </ul>
      )}

      <ConfirmDialog
        open={toDelete !== null}
        title="Delete medicine?"
        confirmLabel="Delete medicine"
        busy={deleting}
        error={deleteError}
        onCancel={() => setToDelete(null)}
        onConfirm={() => void onDelete()}
      >
        <p>
          <strong className="font-semibold">{toDelete?.name}</strong> will be removed from the list.
          {toDelete && toDelete.visit_count > 0 && " It was given in past visits, so it can't be deleted — rename it instead."}
        </p>
      </ConfirmDialog>
    </Card>
  );
}
