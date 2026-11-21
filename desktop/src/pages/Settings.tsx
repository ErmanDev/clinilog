import { Check, Loader2, Pencil, Plus, Trash2, X } from "lucide-react";
import { useState, type FormEvent } from "react";
import { ConfirmDialog } from "../components/Dialog";
import { buttonClass, ErrorNotice, inputClass, SelectField, TextField } from "../components/form";
import { Card } from "../components/ui";
import { asCmdError, sectionsApi, type Section } from "../lib/api";
import { sectionLabel } from "../lib/format";
import { usePlan } from "../lib/session";
import { groupByGrade, useSections } from "../lib/useSections";
import { validateSectionName } from "../lib/validation";
import { MedicinesManager } from "./MedicinesManager";

const GRADES = Array.from({ length: 12 }, (_, i) => i + 1);

export function SettingsPage() {
  return (
    <div className="max-w-3xl space-y-6">
      <SectionsManager />
      <MedicinesManager />
    </div>
  );
}

function SectionsManager() {
  const { readOnly } = usePlan();
  const { sections, error, reload } = useSections();

  // Add form
  const [grade, setGrade] = useState(7);
  const [name, setName] = useState("");
  const [nameTouched, setNameTouched] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);
  const [adding, setAdding] = useState(false);

  // Inline rename
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editName, setEditName] = useState("");
  const [editError, setEditError] = useState<string | null>(null);
  const [savingEdit, setSavingEdit] = useState(false);

  // Delete
  const [toDelete, setToDelete] = useState<Section | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [deleteError, setDeleteError] = useState<string | null>(null);

  const nameError = validateSectionName(name);

  async function onAdd(e: FormEvent) {
    e.preventDefault();
    setNameTouched(true);
    if (nameError) return;
    setAdding(true);
    setAddError(null);
    try {
      await sectionsApi.create({ grade_level: grade, section_name: name });
      setName("");
      setNameTouched(false);
      await reload();
    } catch (err) {
      const ce = asCmdError(err);
      setAddError(ce.fields?.section_name ?? ce.fields?.grade_level ?? ce.message);
    } finally {
      setAdding(false);
    }
  }

  async function onRename(s: Section) {
    const msg = validateSectionName(editName);
    if (msg) return setEditError(msg);
    setSavingEdit(true);
    setEditError(null);
    try {
      await sectionsApi.update(s.id, { grade_level: s.grade_level, section_name: editName });
      setEditingId(null);
      await reload();
    } catch (err) {
      const ce = asCmdError(err);
      setEditError(ce.fields?.section_name ?? ce.message);
    } finally {
      setSavingEdit(false);
    }
  }

  async function onDelete() {
    if (!toDelete) return;
    setDeleting(true);
    setDeleteError(null);
    try {
      await sectionsApi.remove(toDelete.id);
      setToDelete(null);
      await reload();
    } catch (err) {
      setDeleteError(asCmdError(err).message);
    } finally {
      setDeleting(false);
    }
  }

  return (
    <Card title="Sections">
      <p className="-mt-2 mb-4 text-sm text-slate-500">Students are assigned to one of these sections.</p>

      {!readOnly && (
        <form onSubmit={onAdd} noValidate className="mb-5 flex flex-wrap items-start gap-3 rounded-lg bg-slate-50 p-4">
          <div className="w-32">
            <SelectField label="Grade" value={grade} onChange={(e) => setGrade(Number(e.target.value))}>
              {GRADES.map((g) => (
                <option key={g} value={g}>
                  Grade {g}
                </option>
              ))}
            </SelectField>
          </div>
          <div className="min-w-48 flex-1">
            <TextField
              label="Section name"
              value={name}
              maxLength={50}
              onChange={(e) => {
                setName(e.target.value);
                setAddError(null);
              }}
              onBlur={() => setNameTouched(true)}
              error={addError ?? (nameTouched && name !== "" ? nameError : null)}
              placeholder="e.g. Sampaguita"
            />
          </div>
          <button type="submit" className={`${buttonClass("primary")} mt-6`} disabled={!!nameError || adding}>
            {adding ? <Loader2 className="size-4 animate-spin" aria-hidden /> : <Plus className="size-4" aria-hidden />}
            Add section
          </button>
        </form>
      )}

      {error && <ErrorNotice>{error}</ErrorNotice>}
      {!sections && !error && (
        <p className="flex items-center gap-2 text-sm text-slate-500">
          <Loader2 className="size-4 animate-spin" aria-hidden /> Loading…
        </p>
      )}
      {sections?.length === 0 && <p className="text-sm text-slate-500">No sections yet.</p>}

      <div className="space-y-5">
        {sections &&
          groupByGrade(sections).map(([g, list]) => (
            <div key={g}>
              <h3 className="mb-1.5 text-xs font-semibold tracking-wide text-slate-500 uppercase">Grade {g}</h3>
              <ul className="divide-y divide-slate-100 rounded-lg border border-slate-200">
                {list.map((s) => (
                  <li key={s.id} className="flex items-center gap-3 px-3 py-2">
                    {editingId === s.id ? (
                      <div className="flex-1">
                        <label htmlFor={`rename-${s.id}`} className="sr-only">
                          New name for {sectionLabel(s)}
                        </label>
                        <input
                          id={`rename-${s.id}`}
                          value={editName}
                          maxLength={50}
                          autoFocus
                          onChange={(e) => {
                            setEditName(e.target.value);
                            setEditError(null);
                          }}
                          onKeyDown={(e) => {
                            if (e.key === "Enter") void onRename(s);
                            if (e.key === "Escape") setEditingId(null);
                          }}
                          className={inputClass(!!editError, "mt-0 py-1.5")}
                          aria-invalid={!!editError}
                        />
                        {editError && <p className="mt-1 text-xs font-medium text-red-700">{editError}</p>}
                      </div>
                    ) : (
                      <span className="flex-1 text-sm font-medium">{s.section_name}</span>
                    )}
                    <span className="text-xs text-slate-500 tabular-nums">
                      {s.student_count} student{s.student_count === 1 ? "" : "s"}
                    </span>
                    {!readOnly &&
                      (editingId === s.id ? (
                        <>
                          <button
                            type="button"
                            className={buttonClass("ghost", "sm")}
                            onClick={() => void onRename(s)}
                            disabled={savingEdit}
                            aria-label="Save name"
                          >
                            {savingEdit ? <Loader2 className="size-4 animate-spin" /> : <Check className="size-4" />}
                          </button>
                          <button
                            type="button"
                            className={buttonClass("ghost", "sm")}
                            onClick={() => setEditingId(null)}
                            aria-label="Cancel rename"
                          >
                            <X className="size-4" />
                          </button>
                        </>
                      ) : (
                        <>
                          <button
                            type="button"
                            className={buttonClass("ghost", "sm")}
                            onClick={() => {
                              setEditingId(s.id);
                              setEditName(s.section_name);
                              setEditError(null);
                            }}
                            aria-label={`Rename ${sectionLabel(s)}`}
                          >
                            <Pencil className="size-4" />
                          </button>
                          <button
                            type="button"
                            className={buttonClass("ghost", "sm")}
                            onClick={() => {
                              setDeleteError(null);
                              setToDelete(s);
                            }}
                            aria-label={`Delete ${sectionLabel(s)}`}
                          >
                            <Trash2 className="size-4 text-red-600" />
                          </button>
                        </>
                      ))}
                  </li>
                ))}
              </ul>
            </div>
          ))}
      </div>

      <ConfirmDialog
        open={toDelete !== null}
        title="Delete section?"
        confirmLabel="Delete section"
        busy={deleting}
        error={deleteError}
        onCancel={() => setToDelete(null)}
        onConfirm={() => void onDelete()}
      >
        <p>
          <strong className="font-semibold">{toDelete && sectionLabel(toDelete)}</strong> will be removed.
          {toDelete && toDelete.student_count > 0 && " It still has students, so you'll need to move them first."}
        </p>
      </ConfirmDialog>
    </Card>
  );
}
