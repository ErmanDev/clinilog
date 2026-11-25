import { ChevronLeft, ChevronRight, ChevronsLeft, ChevronsRight } from "lucide-react";
import { useId } from "react";

/** Page numbers to show around the current one, with null for a gap: 1 … 4 5 [6] 7 8 … 20 */
export function pageList(page: number, last: number): (number | null)[] {
  if (last <= 7) return Array.from({ length: last }, (_, i) => i + 1);
  const start = Math.max(2, Math.min(page - 1, last - 4));
  const end = Math.min(last - 1, Math.max(page + 1, 5));
  const middle = Array.from({ length: end - start + 1 }, (_, i) => start + i);
  return [1, ...(start > 2 ? [null] : []), ...middle, ...(end < last - 1 ? [null] : []), last];
}

/**
 * Footer for a paged table: "Showing 51–100 of 230", rows per page,
 * and first / previous / numbered / next / last buttons.
 */
export function Pagination({
  page,
  pageSize,
  total,
  onPage,
  onPageSize,
  pageSizes = [25, 50, 100],
  busy = false,
  noun = "entries",
}: {
  page: number;
  pageSize: number;
  total: number;
  onPage: (page: number) => void;
  /** Omit to hide the rows-per-page picker. */
  onPageSize?: (size: number) => void;
  pageSizes?: number[];
  busy?: boolean;
  noun?: string;
}) {
  const sizeId = useId();
  const last = Math.max(1, Math.ceil(total / pageSize));
  const first = total === 0 ? 0 : (page - 1) * pageSize + 1;
  const lastShown = Math.min(total, page * pageSize);

  const nav = "inline-flex size-8 items-center justify-center rounded-md text-slate-600 hover:bg-slate-100 disabled:pointer-events-none disabled:text-slate-300";

  return (
    <div className="flex flex-wrap items-center justify-between gap-3 border-t border-slate-200 px-4 py-3 text-sm text-slate-600">
      <div className="flex items-center gap-4">
        <p className="tabular-nums">
          {total === 0 ? (
            `No ${noun}`
          ) : (
            <>
              Showing <span className="font-medium text-slate-900">{first.toLocaleString()}</span>–
              <span className="font-medium text-slate-900">{lastShown.toLocaleString()}</span> of{" "}
              <span className="font-medium text-slate-900">{total.toLocaleString()}</span> {noun}
            </>
          )}
        </p>
        {onPageSize && (
          <label htmlFor={sizeId} className="flex items-center gap-2 text-xs text-slate-500">
            Rows per page
            <select
              id={sizeId}
              value={pageSize}
              onChange={(e) => onPageSize(Number(e.target.value))}
              className="rounded-md border border-slate-300 bg-white px-2 py-1 text-sm text-slate-700 outline-none focus:border-teal-600 focus:ring-2 focus:ring-teal-100"
            >
              {pageSizes.map((s) => (
                <option key={s} value={s}>
                  {s}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>

      {last > 1 && (
        <nav aria-label="Pages" className="flex items-center gap-0.5">
          <button type="button" className={nav} disabled={page <= 1 || busy} onClick={() => onPage(1)} aria-label="First page">
            <ChevronsLeft className="size-4" aria-hidden />
          </button>
          <button type="button" className={nav} disabled={page <= 1 || busy} onClick={() => onPage(page - 1)} aria-label="Previous page">
            <ChevronLeft className="size-4" aria-hidden />
          </button>
          {pageList(page, last).map((p, i) =>
            p === null ? (
              <span key={`gap-${i}`} className="px-1 text-slate-400" aria-hidden>
                …
              </span>
            ) : (
              <button
                key={p}
                type="button"
                onClick={() => onPage(p)}
                disabled={busy && p !== page}
                aria-current={p === page ? "page" : undefined}
                aria-label={`Page ${p}`}
                className={`inline-flex h-8 min-w-8 items-center justify-center rounded-md px-2 tabular-nums ${
                  p === page ? "bg-teal-700 font-semibold text-white" : "text-slate-700 hover:bg-slate-100"
                }`}
              >
                {p}
              </button>
            ),
          )}
          <button type="button" className={nav} disabled={page >= last || busy} onClick={() => onPage(page + 1)} aria-label="Next page">
            <ChevronRight className="size-4" aria-hidden />
          </button>
          <button type="button" className={nav} disabled={page >= last || busy} onClick={() => onPage(last)} aria-label="Last page">
            <ChevronsRight className="size-4" aria-hidden />
          </button>
        </nav>
      )}
    </div>
  );
}
