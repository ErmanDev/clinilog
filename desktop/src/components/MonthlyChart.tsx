import { useState } from "react";

const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const MONTHS_LONG = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];

/** Rounds up to 1, 2 or 5 × 10ⁿ so the axis has tidy ticks. */
function niceMax(n: number): number {
  if (n <= 4) return 4;
  const pow = 10 ** Math.floor(Math.log10(n));
  for (const m of [1, 2, 2.5, 5, 10]) if (m * pow >= n) return m * pow;
  return 10 * pow;
}

/**
 * Single-series column chart of visits per month. One hue (no legend: the card
 * title names the series); the selected month, if any, is emphasised. Every bar
 * has a hover/focus tooltip, and the table below carries every value (also for print).
 */
export function MonthlyChart({
  data,
  year,
  selectedMonth,
}: {
  data: { month: number; visits: number }[];
  year: number;
  selectedMonth?: number;
}) {
  const [active, setActive] = useState<number | null>(null);
  const max = niceMax(Math.max(0, ...data.map((d) => d.visits)));
  const ticks = [0, max / 4, max / 2, (3 * max) / 4, max];

  return (
    <figure className="break-inside-avoid">
      <div className="flex gap-2 pt-3">
        {/* y axis labels */}
        <div className="relative h-48 w-8 shrink-0 text-right text-[11px] text-slate-500 tabular-nums" aria-hidden>
          {ticks.map((t) => (
            <span key={t} className="absolute right-0 -translate-y-1/2" style={{ bottom: `${(t / max) * 100}%` }}>
              {Number.isInteger(t) ? t : ""}
            </span>
          ))}
        </div>

        <div className="relative h-48 flex-1">
          {/* recessive grid */}
          {ticks.map((t) => (
            <div
              key={t}
              className={`absolute inset-x-0 border-t ${t === 0 ? "border-slate-400" : "border-slate-200"}`}
              style={{ bottom: `${(t / max) * 100}%` }}
              aria-hidden
            />
          ))}

          <div className="absolute inset-0 flex items-end gap-[2px]" role="list" aria-label={`Visits per month, ${year}`}>
            {data.map((d) => {
              const pct = (d.visits / max) * 100;
              const dim = selectedMonth !== undefined && selectedMonth !== d.month;
              const label = `${MONTHS_LONG[d.month - 1]} ${year}: ${d.visits} visit${d.visits === 1 ? "" : "s"}`;
              return (
                <div
                  key={d.month}
                  role="listitem"
                  tabIndex={0}
                  aria-label={label}
                  onMouseEnter={() => setActive(d.month)}
                  onMouseLeave={() => setActive(null)}
                  onFocus={() => setActive(d.month)}
                  onBlur={() => setActive(null)}
                  className="relative flex h-full flex-1 items-end justify-center outline-none focus-visible:bg-teal-50/60"
                >
                  {d.visits > 0 && (
                    <div
                      className={`w-full max-w-10 rounded-t ${dim ? "bg-teal-600/35" : "bg-teal-600"} ${active === d.month ? "brightness-90" : ""}`}
                      style={{ height: `${pct}%` }}
                    />
                  )}
                  {active === d.month && (
                    <div className="pointer-events-none absolute bottom-full z-10 mb-1 rounded-md bg-slate-900 px-2 py-1 text-xs whitespace-nowrap text-white shadow print:hidden">
                      {label}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        </div>
      </div>

      {/* month labels */}
      <div className="mt-1 flex gap-[2px] pl-10 text-center text-[11px] text-slate-500" aria-hidden>
        {MONTHS.map((m, i) => (
          <span key={m} className={`flex-1 ${selectedMonth === i + 1 ? "font-semibold text-slate-900" : ""}`}>
            {m}
          </span>
        ))}
      </div>

      {/* table view: every value, for screen readers and print */}
      <table className="mt-4 w-full table-fixed border-t border-slate-200 text-center text-xs">
        <caption className="sr-only">Visits per month, {year}</caption>
        <thead>
          <tr className="text-slate-500">
            {MONTHS.map((m) => (
              <th key={m} scope="col" className="pt-2 font-medium">
                {m}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          <tr className="text-slate-900 tabular-nums">
            {data.map((d) => (
              <td key={d.month} className={`py-1 ${selectedMonth === d.month ? "font-semibold" : ""}`}>
                {d.visits}
              </td>
            ))}
          </tr>
        </tbody>
      </table>
    </figure>
  );
}

export { MONTHS_LONG };
