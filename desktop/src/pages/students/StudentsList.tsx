import { ChevronLeft, ChevronRight, Loader2, Plus, Search, Users } from "lucide-react";
import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { buttonClass, ErrorNotice, inputClass } from "../../components/form";
import { asCmdError, studentsApi, type StudentPage } from "../../lib/api";
import { sectionLabel } from "../../lib/format";
import { StudentTable } from "../../components/StudentTable";
import { usePlan } from "../../lib/session";
import { groupByGrade, useSections } from "../../lib/useSections";

const PAGE_SIZE = 25;

export function StudentsListPage() {
  const navigate = useNavigate();
  const { readOnly, info } = usePlan();
  const { sections } = useSections();

  const [q, setQ] = useState("");
  const [debouncedQ, setDebouncedQ] = useState("");
  const [sectionId, setSectionId] = useState("");
  const [page, setPage] = useState(1);
  const [data, setData] = useState<StudentPage | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const t = window.setTimeout(() => setDebouncedQ(q.trim()), 300);
    return () => window.clearTimeout(t);
  }, [q]);

  useEffect(() => setPage(1), [debouncedQ, sectionId]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    studentsApi
      .list({ q: debouncedQ, section_id: sectionId || null, page, page_size: PAGE_SIZE })
      .then((d) => !cancelled && (setData(d), setError(null)))
      .catch((e) => !cancelled && setError(asCmdError(e).message))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [debouncedQ, sectionId, page]);

  const filtered = debouncedQ !== "" || sectionId !== "";
  const maxStudents = info?.limits.max_students ?? null;
  const atLimit = !filtered && maxStudents !== null && data !== null && data.total >= maxStudents;
  const addDisabledReason = readOnly
    ? "Adding students is paused while the subscription is inactive."
    : atLimit
      ? `Your Basic plan allows up to ${maxStudents} students.`
      : null;

  const from = data && data.total > 0 ? (data.page - 1) * data.page_size + 1 : 0;
  const to = data ? Math.min(data.page * data.page_size, data.total) : 0;
  const lastPage = data ? Math.max(1, Math.ceil(data.total / data.page_size)) : 1;

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-end gap-3">
        <div className="relative min-w-64 flex-1">
          <label htmlFor="student-search" className="sr-only">
            Search students
          </label>
          <Search className="pointer-events-none absolute top-1/2 left-3 mt-0.5 size-4 -translate-y-1/2 text-slate-400" aria-hidden />
          <input
            id="student-search"
            type="search"
            value={q}
            onChange={(e) => setQ(e.target.value)}
            placeholder="Search by name or LRN"
            maxLength={100}
            className={inputClass(false, "pl-9")}
          />
        </div>
        <div className="w-60">
          <label htmlFor="section-filter" className="sr-only">
            Filter by section
          </label>
          <select
            id="section-filter"
            value={sectionId}
            onChange={(e) => setSectionId(e.target.value)}
            className={inputClass(false)}
          >
            <option value="">All sections</option>
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
          </select>
        </div>
        <button
          type="button"
          className={`${buttonClass("primary")} mt-1`}
          disabled={!!addDisabledReason}
          title={addDisabledReason ?? undefined}
          onClick={() => navigate("/students/new")}
        >
          <Plus className="size-4" aria-hidden />
          Add student
        </button>
      </div>

      {addDisabledReason && atLimit && (
        <p className="text-sm text-amber-800">
          {addDisabledReason} Upgrade to Premium for unlimited student records.
        </p>
      )}

      {error && <ErrorNotice>{error}</ErrorNotice>}

      <div className="overflow-hidden rounded-xl border border-slate-200 bg-white shadow-sm">
        <StudentTable students={data?.items ?? []} />

        {loading && !data && (
          <div className="flex items-center justify-center gap-2 py-12 text-sm text-slate-500">
            <Loader2 className="size-4 animate-spin" aria-hidden /> Loading students…
          </div>
        )}

        {data && data.items.length === 0 && (
          <div className="flex flex-col items-center py-14 text-center">
            <Users className="size-8 text-slate-300" aria-hidden />
            <p className="mt-3 text-sm font-medium text-slate-700">
              {filtered ? "No students match your search." : "No students yet."}
            </p>
            {!filtered && !addDisabledReason && (
              <Link to="/students/new" className={`${buttonClass("secondary", "sm")} mt-4`}>
                Add the first student
              </Link>
            )}
          </div>
        )}

        {data && data.total > 0 && (
          <div className="flex items-center justify-between border-t border-slate-200 px-4 py-3 text-sm text-slate-600">
            <p aria-live="polite">
              {from}–{to} of {data.total} student{data.total === 1 ? "" : "s"}
              {!filtered && maxStudents !== null && <span className="text-slate-400"> · plan limit {maxStudents}</span>}
            </p>
            <div className="flex items-center gap-1">
              <button
                type="button"
                className={buttonClass("ghost", "sm")}
                disabled={page <= 1 || loading}
                onClick={() => setPage((p) => p - 1)}
                aria-label="Previous page"
              >
                <ChevronLeft className="size-4" aria-hidden />
              </button>
              <span className="px-2 tabular-nums">
                Page {page} of {lastPage}
              </span>
              <button
                type="button"
                className={buttonClass("ghost", "sm")}
                disabled={page >= lastPage || loading}
                onClick={() => setPage((p) => p + 1)}
                aria-label="Next page"
              >
                <ChevronRight className="size-4" aria-hidden />
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
