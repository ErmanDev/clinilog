import { ChevronLeft, ChevronRight, Loader2, Search as SearchIcon, X } from "lucide-react";
import { useCallback, useEffect, useMemo, useState, type KeyboardEvent, type ReactNode } from "react";
import { useSearchParams } from "react-router-dom";
import { buttonClass, ErrorNotice, inputClass } from "../components/form";
import { PremiumBadge, PremiumGate } from "../components/PremiumGate";
import { StudentTable } from "../components/StudentTable";
import { Card } from "../components/ui";
import { asCmdError, studentsApi, visitsApi, type StudentPage, type VisitPage, type VisitSearch } from "../lib/api";
import { sectionLabel } from "../lib/format";
import { usePlan, useSession } from "../lib/session";
import { groupByGrade, useSections } from "../lib/useSections";
import { todayPH } from "../lib/validation";
import { VisitTable } from "./visits/VisitsList";

type Tab = "students" | "visits";
const PAGE_SIZE = 25;

/**
 * Search. Basic filters (name/LRN, section) for every plan; advanced filters
 * (grade, visit date range, complaint) for Premium. Filters live in the URL so
 * going back from a result restores the search.
 */
export function SearchPage() {
  const [params, setParams] = useSearchParams();
  const { features } = usePlan();
  const { refreshPlan } = useSession();
  const { sections } = useSections();

  const tab: Tab = params.get("tab") === "visits" ? "visits" : "students";
  const f = {
    q: params.get("q") ?? "",
    section: params.get("section") ?? "",
    grade: params.get("grade") ?? "",
    from: params.get("from") ?? "",
    to: params.get("to") ?? "",
    complaint: params.get("complaint") ?? "",
  };
  const page = Math.max(1, Number(params.get("page")) || 1);

  // Typing updates local text at once; the URL (and the search) follows after a pause.
  const [qText, setQText] = useState(f.q);
  const [complaintText, setComplaintText] = useState(f.complaint);

  // Applies a change to the *current* URL (not a copy from an earlier render),
  // so a debounced update can't undo a filter changed in the meantime.
  const set = useCallback(
    (patch: Record<string, string>, keepPage = false) => {
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          for (const [k, v] of Object.entries(patch)) {
            if (v) next.set(k, v);
            else next.delete(k);
          }
          if (!keepPage) next.delete("page");
          return next;
        },
        { replace: true },
      );
    },
    [setParams],
  );

  useEffect(() => {
    const v = qText.trim();
    if (v === f.q) return;
    const t = window.setTimeout(() => set({ q: v }), 300);
    return () => window.clearTimeout(t);
  }, [qText, f.q, set]);
  useEffect(() => {
    const v = complaintText.trim();
    if (v === f.complaint) return;
    const t = window.setTimeout(() => set({ complaint: v }), 400);
    return () => window.clearTimeout(t);
  }, [complaintText, f.complaint, set]);

  // Advanced filters are only sent on Premium (the server would refuse them anyway).
  const adv = features.advanced_search;
  const grade = adv && f.grade ? Number(f.grade) : undefined;
  const visitFilters: VisitSearch = useMemo(
    () => ({
      q: f.q || undefined,
      section_id: f.section || undefined,
      grade_level: grade,
      date_from: adv ? f.from || undefined : undefined,
      date_to: adv ? f.to || undefined : undefined,
      complaint: adv ? f.complaint || undefined : undefined,
    }),
    [f.q, f.section, grade, adv, f.from, f.to, f.complaint],
  );
  const hasFilter =
    tab === "students"
      ? !!(f.q || f.section || grade)
      : Object.values(visitFilters).some((v) => v !== undefined);
  const rangeError = adv && f.from && f.to && f.from > f.to ? "The 'from' date must be on or before the 'to' date." : null;

  const [students, setStudents] = useState<StudentPage | null>(null);
  const [visits, setVisits] = useState<VisitPage | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setError(null);
    if (!hasFilter || rangeError) {
      setStudents(null);
      setVisits(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    const run =
      tab === "students"
        ? studentsApi
            .list({ q: f.q || undefined, section_id: f.section || null, grade_level: grade, page, page_size: PAGE_SIZE })
            .then((r) => !cancelled && setStudents(r))
        : visitsApi.list({ ...visitFilters, page, page_size: PAGE_SIZE }).then((r) => !cancelled && setVisits(r));
    run
      .catch((e) => {
        if (cancelled) return;
        const err = asCmdError(e);
        setError(err.message);
        // The plan changed since it was last checked; refresh so the locks update.
        if (err.code === "premium_required") void refreshPlan();
      })
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [tab, hasFilter, rangeError, f.q, f.section, grade, visitFilters, page, refreshPlan]);

  const result = tab === "students" ? students : visits;
  const lastPage = result ? Math.max(1, Math.ceil(result.total / result.page_size)) : 1;
  const sectionOptions = sections ? groupByGrade(sections).filter(([g]) => !grade || g === grade) : [];
  const grades = sections ? [...new Set(sections.map((s) => s.grade_level))].sort((a, b) => a - b) : [];

  function clearAll() {
    setQText("");
    setComplaintText("");
    setParams(tab === "visits" ? { tab } : {}, { replace: true });
  }

  function onTabKey(e: KeyboardEvent) {
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      e.preventDefault();
      set({ tab: tab === "students" ? "visits" : "" });
    }
  }

  return (
    <div className="space-y-5">
      <div role="tablist" aria-label="What to search" className="inline-flex rounded-lg bg-slate-200/70 p-1" onKeyDown={onTabKey}>
        {(["students", "visits"] as const).map((t) => (
          <button
            key={t}
            type="button"
            role="tab"
            id={`tab-${t}`}
            aria-selected={tab === t}
            aria-controls="search-results"
            tabIndex={tab === t ? 0 : -1}
            onClick={() => set({ tab: t === "visits" ? "visits" : "" })}
            className={`rounded-md px-4 py-1.5 text-sm font-medium transition-colors ${
              tab === t ? "bg-white text-slate-900 shadow-sm" : "text-slate-600 hover:text-slate-900"
            }`}
          >
            {t === "students" ? "Students" : "Clinic visits"}
          </button>
        ))}
      </div>

      <Card title={tab === "students" ? "Find students" : "Find clinic visits"}>
        <div className="grid gap-4 md:grid-cols-2">
          <Labeled label="Student name or LRN" htmlFor="s-q">
            <div className="relative">
              <SearchIcon className="pointer-events-none absolute top-1/2 left-3 mt-0.5 size-4 -translate-y-1/2 text-slate-400" aria-hidden />
              <input
                id="s-q"
                type="search"
                value={qText}
                maxLength={100}
                autoFocus
                onChange={(e) => setQText(e.target.value)}
                placeholder="e.g. Dela Cruz, Juan"
                className={inputClass(false, "pl-9")}
              />
            </div>
          </Labeled>
          <Labeled label="Section" htmlFor="s-section">
            <select id="s-section" value={f.section} onChange={(e) => set({ section: e.target.value })} className={inputClass(false)}>
              <option value="">Any section</option>
              {sectionOptions.map(([g, list]) => (
                <optgroup key={g} label={`Grade ${g}`}>
                  {list.map((s) => (
                    <option key={s.id} value={s.id}>
                      {sectionLabel(s)}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
          </Labeled>
        </div>
      </Card>

      <PremiumGate feature="advanced_search" label="Advanced filters">
        <Card title="Advanced filters" action={!adv ? <PremiumBadge /> : undefined}>
          <div className="grid gap-4 md:grid-cols-4">
            <Labeled label="Grade" htmlFor="s-grade">
              <select
                id="s-grade"
                value={f.grade}
                onChange={(e) => {
                  // A section from another grade no longer fits: clear it.
                  const g = e.target.value;
                  const sec = sections?.find((s) => s.id === f.section);
                  set({ grade: g, ...(sec && g && sec.grade_level !== Number(g) ? { section: "" } : {}) });
                }}
                className={inputClass(false)}
              >
                <option value="">Any grade</option>
                {grades.map((g) => (
                  <option key={g} value={g}>
                    Grade {g}
                  </option>
                ))}
              </select>
            </Labeled>
            {tab === "visits" && (
              <>
                <Labeled label="Visit date from" htmlFor="s-from">
                  <input
                    id="s-from"
                    type="date"
                    value={f.from}
                    max={f.to || todayPH()}
                    onChange={(e) => set({ from: e.target.value })}
                    className={inputClass(!!rangeError)}
                  />
                </Labeled>
                <Labeled label="Visit date to" htmlFor="s-to">
                  <input
                    id="s-to"
                    type="date"
                    value={f.to}
                    min={f.from || undefined}
                    max={todayPH()}
                    onChange={(e) => set({ to: e.target.value })}
                    className={inputClass(!!rangeError)}
                  />
                </Labeled>
                <Labeled label="Complaint has words" htmlFor="s-complaint">
                  <input
                    id="s-complaint"
                    type="search"
                    value={complaintText}
                    maxLength={100}
                    onChange={(e) => setComplaintText(e.target.value)}
                    placeholder="e.g. headache"
                    className={inputClass(false)}
                  />
                </Labeled>
              </>
            )}
          </div>
          {rangeError && <p className="mt-2 text-xs font-medium text-red-700">{rangeError}</p>}
          {tab === "students" && adv && (
            <p className="mt-3 text-xs text-slate-500">Date range and complaint filters are on the Clinic visits tab.</p>
          )}
        </Card>
      </PremiumGate>

      <section id="search-results" role="tabpanel" aria-labelledby={`tab-${tab}`} className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        <div className="flex items-center justify-between border-b border-slate-200 px-4 py-3">
          <h2 className="text-sm font-semibold text-slate-800" aria-live="polite">
            {!hasFilter
              ? "Results"
              : result
                ? `${result.total} ${tab === "students" ? "student" : "visit"}${result.total === 1 ? "" : "s"} found`
                : "Searching…"}
          </h2>
          {hasFilter && (
            <button type="button" className={buttonClass("ghost", "sm")} onClick={clearAll}>
              <X className="size-3.5" aria-hidden /> Clear filters
            </button>
          )}
        </div>

        {error && (
          <div className="p-4">
            <ErrorNotice>{error}</ErrorNotice>
          </div>
        )}

        {!hasFilter && (
          <Empty>Type a name or LRN, or choose a filter, to search {tab === "students" ? "students" : "clinic visits"}.</Empty>
        )}
        {hasFilter && loading && !result && (
          <p className="flex items-center justify-center gap-2 py-12 text-sm text-slate-500">
            <Loader2 className="size-4 animate-spin" aria-hidden /> Searching…
          </p>
        )}
        {hasFilter && result && result.total === 0 && <Empty>Nothing matches these filters.</Empty>}

        {hasFilter && tab === "students" && students && <StudentTable students={students.items} />}
        {hasFilter && tab === "visits" && visits && <VisitTable visits={visits.items} showDate />}

        {hasFilter && result && result.total > result.page_size && (
          <div className="flex items-center justify-end gap-1 border-t border-slate-200 px-4 py-3 text-sm text-slate-600">
            <button type="button" className={buttonClass("ghost", "sm")} disabled={page <= 1 || loading} onClick={() => set({ page: String(page - 1) }, true)} aria-label="Previous page">
              <ChevronLeft className="size-4" aria-hidden />
            </button>
            <span className="px-2 tabular-nums">
              Page {page} of {lastPage}
            </span>
            <button type="button" className={buttonClass("ghost", "sm")} disabled={page >= lastPage || loading} onClick={() => set({ page: String(page + 1) }, true)} aria-label="Next page">
              <ChevronRight className="size-4" aria-hidden />
            </button>
          </div>
        )}
      </section>
    </div>
  );
}

function Labeled({ label, htmlFor, children }: { label: string; htmlFor: string; children: ReactNode }) {
  return (
    <div>
      <label htmlFor={htmlFor} className="block text-xs font-medium text-slate-600">
        {label}
      </label>
      {children}
    </div>
  );
}

function Empty({ children }: { children: ReactNode }) {
  return <p className="px-4 py-12 text-center text-sm text-slate-500">{children}</p>;
}
