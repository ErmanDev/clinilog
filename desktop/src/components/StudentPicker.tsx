import { Loader2, Search, X } from "lucide-react";
import { useEffect, useId, useRef, useState } from "react";
import { asCmdError, studentsApi, syncApi, type Student } from "../lib/api";
import { listName, sectionLabel } from "../lib/format";
import { useSync } from "../lib/sync";
import { inputClass } from "./form";

export interface PickedStudent {
  id: string;
  lrn: string;
  first_name: string;
  middle_name: string | null;
  last_name: string;
  grade_level: number;
  section_name: string;
}

export function toPicked(s: Student): PickedStudent {
  return {
    id: s.id,
    lrn: s.lrn,
    first_name: s.first_name,
    middle_name: s.middle_name,
    last_name: s.last_name,
    grade_level: s.section.grade_level,
    section_name: s.section.section_name,
  };
}

/**
 * Search-as-you-type student picker (ARIA combobox). Type a name or LRN,
 * use ↑/↓ and Enter, or click. Shows the chosen student as a chip.
 */
export function StudentPicker({
  value,
  onChange,
  error,
  disabled,
  autoFocus,
}: {
  value: PickedStudent | null;
  onChange: (s: PickedStudent | null) => void;
  error?: string | null;
  disabled?: boolean;
  autoFocus?: boolean;
}) {
  const id = useId();
  const listId = `${id}-list`;
  const errId = `${id}-err`;
  const inputRef = useRef<HTMLInputElement>(null);

  const [q, setQ] = useState("");
  const [open, setOpen] = useState(false);
  const [results, setResults] = useState<PickedStudent[]>([]);
  const [fromCache, setFromCache] = useState(false);
  const [loading, setLoading] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [active, setActive] = useState(0);
  // Known to be offline: search the saved list straight away instead of waiting for a timeout.
  const { status } = useSync();
  const offline = !!status && (!status.online || status.session_offline);

  useEffect(() => {
    const term = q.trim();
    if (!term) {
      setResults([]);
      return;
    }
    let cancelled = false;
    setLoading(true);
    const t = window.setTimeout(() => {
      const fromServer = offline
        ? Promise.reject({ kind: "network", message: "offline" })
        : studentsApi.list({ q: term, page_size: 8 }).then((p) => p.items.map(toPicked));
      fromServer
        .then((items) => ({ items, cached: false }))
        .catch(async (e) => {
          // Server unreachable: search the student list saved on this PC instead.
          if (asCmdError(e).kind !== "network") throw e;
          return { items: await syncApi.cachedStudents(term), cached: true };
        })
        .then(({ items, cached }) => {
          if (cancelled) return;
          setResults(items);
          setFromCache(cached);
          setActive(0);
          setSearchError(null);
        })
        .catch((e) => !cancelled && setSearchError(asCmdError(e).message))
        .finally(() => !cancelled && setLoading(false));
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(t);
    };
  }, [q, offline]);

  function pick(s: PickedStudent) {
    onChange(s);
    setQ("");
    setOpen(false);
  }

  if (value) {
    return (
      <div>
        <p className="block text-sm font-medium text-slate-700">Student</p>
        <div className="mt-1 flex items-center justify-between gap-3 rounded-lg border border-teal-200 bg-teal-50 px-3 py-2">
          <div className="min-w-0">
            <p className="truncate text-sm font-semibold text-teal-900">{listName(value)}</p>
            <p className="text-xs text-teal-800">
              LRN <span className="font-mono">{value.lrn}</span> · {sectionLabel(value)}
            </p>
          </div>
          {!disabled && (
            <button
              type="button"
              onClick={() => {
                onChange(null);
                window.setTimeout(() => inputRef.current?.focus(), 0);
              }}
              className="rounded-md p-1 text-teal-800 hover:bg-teal-100"
              aria-label="Choose a different student"
            >
              <X className="size-4" aria-hidden />
            </button>
          )}
        </div>
      </div>
    );
  }

  const showList = open && q.trim() !== "";

  return (
    <div className="relative">
      <label htmlFor={id} className="block text-sm font-medium text-slate-700">
        Student
      </label>
      <div className="relative">
        <Search className="pointer-events-none absolute top-1/2 left-3 mt-0.5 size-4 -translate-y-1/2 text-slate-400" aria-hidden />
        <input
          ref={inputRef}
          id={id}
          role="combobox"
          aria-expanded={showList}
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={showList && results[active] ? `${id}-opt-${active}` : undefined}
          aria-invalid={!!error}
          aria-describedby={error ? errId : undefined}
          value={q}
          disabled={disabled}
          autoFocus={autoFocus}
          autoComplete="off"
          maxLength={100}
          placeholder="Type a name or LRN"
          onChange={(e) => {
            setQ(e.target.value);
            setOpen(true);
          }}
          onFocus={() => setOpen(true)}
          onBlur={() => window.setTimeout(() => setOpen(false), 150)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setOpen(true);
              setActive((a) => Math.min(a + 1, results.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActive((a) => Math.max(a - 1, 0));
            } else if (e.key === "Enter" && showList && results[active]) {
              e.preventDefault();
              pick(results[active]);
            } else if (e.key === "Escape") {
              setOpen(false);
            }
          }}
          className={inputClass(!!error, "pl-9")}
        />
        {loading && (
          <Loader2 className="absolute top-1/2 right-3 mt-0.5 size-4 -translate-y-1/2 animate-spin text-slate-400" aria-hidden />
        )}
      </div>
      {error && (
        <p id={errId} className="mt-1.5 text-xs font-medium text-red-700">
          {error}
        </p>
      )}

      {showList && (
        <ul
          id={listId}
          role="listbox"
          className="absolute z-10 mt-1 max-h-72 w-full overflow-y-auto rounded-lg border border-slate-200 bg-white py-1 shadow-lg"
        >
          {searchError && <li className="px-3 py-2 text-sm text-red-700">{searchError}</li>}
          {fromCache && !searchError && (
            <li className="border-b border-slate-100 px-3 py-1.5 text-xs text-slate-500">Offline — searching the list saved on this PC</li>
          )}
          {!searchError && !loading && results.length === 0 && (
            <li className="px-3 py-2 text-sm text-slate-500">No students match “{q.trim()}”.</li>
          )}
          {results.map((s, i) => (
            <li
              key={s.id}
              id={`${id}-opt-${i}`}
              role="option"
              aria-selected={i === active}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => pick(s)}
              onMouseEnter={() => setActive(i)}
              className={`cursor-pointer px-3 py-2 ${i === active ? "bg-teal-50" : ""}`}
            >
              <p className="text-sm font-medium">{listName(s)}</p>
              <p className="text-xs text-slate-500">
                LRN <span className="font-mono">{s.lrn}</span> · {sectionLabel(s)}
              </p>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
