import { FileDown, Loader2, Printer } from "lucide-react";
import { useEffect, useState, type ReactNode } from "react";
import { Link } from "react-router-dom";
import { buttonClass, ErrorNotice, inputClass } from "../components/form";
import { MONTHS_LONG, MonthlyChart } from "../components/MonthlyChart";
import { PremiumBadge, PremiumGate } from "../components/PremiumGate";
import { asCmdError, reportsApi, type BasicReport, type DetailedReport } from "../lib/api";
import { formatDate, formatDateTime, listName } from "../lib/format";
import { usePlan, useSession } from "../lib/session";
import { todayPH } from "../lib/validation";

export function ReportsPage() {
  const { session } = useSession();
  const { features } = usePlan();
  const thisYear = Number(todayPH().slice(0, 4));
  const years = Array.from({ length: 5 }, (_, i) => thisYear - i);

  const [year, setYear] = useState(thisYear);
  const [month, setMonth] = useState<number | "">(Number(todayPH().slice(5, 7)));
  const [basic, setBasic] = useState<BasicReport | null>(null);
  const [detailed, setDetailed] = useState<DetailedReport | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [generatedAt, setGeneratedAt] = useState(new Date().toISOString());
  const [saving, setSaving] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setNotice(null);
    const m = month === "" ? undefined : month;
    Promise.all([
      reportsApi.basic(year, m),
      features.detailed_reports ? reportsApi.detailed(year, m) : Promise.resolve(null),
    ])
      .then(([b, d]) => {
        if (cancelled) return;
        setBasic(b);
        setDetailed(d);
        setGeneratedAt(new Date().toISOString());
      })
      .catch((e) => !cancelled && setError(asCmdError(e).message))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [year, month, features.detailed_reports]);

  const periodLabel = month === "" ? `Year ${year}` : `${MONTHS_LONG[month - 1]} ${year}`;
  const fileName = `Clinilog report - ${month === "" ? year : `${year}-${String(month).padStart(2, "0")}`}`;

  async function savePdf() {
    setSaving(true);
    setNotice(null);
    try {
      const path = await reportsApi.savePdf(fileName);
      if (path) setNotice(`Saved to ${path}`);
    } catch (e) {
      setError(asCmdError(e).message);
    } finally {
      setSaving(false);
    }
  }

  const ready = !!basic && !loading;

  return (
    <div className="space-y-5">
      {/* controls (not printed) */}
      <div className="flex flex-wrap items-end justify-between gap-3 print:hidden">
        <div className="flex items-end gap-2">
          <div>
            <label htmlFor="r-year" className="block text-xs font-medium text-slate-600">
              Year
            </label>
            <select id="r-year" value={year} onChange={(e) => setYear(Number(e.target.value))} className={inputClass(false, "w-28")}>
              {years.map((y) => (
                <option key={y} value={y}>
                  {y}
                </option>
              ))}
            </select>
          </div>
          <div>
            <label htmlFor="r-month" className="block text-xs font-medium text-slate-600">
              Month
            </label>
            <select
              id="r-month"
              value={month}
              onChange={(e) => setMonth(e.target.value === "" ? "" : Number(e.target.value))}
              className={inputClass(false, "w-40")}
            >
              <option value="">Whole year</option>
              {MONTHS_LONG.map((m, i) => (
                <option key={m} value={i + 1}>
                  {m}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div className="flex gap-2">
          <button type="button" className={buttonClass("secondary")} onClick={() => window.print()} disabled={!ready}>
            <Printer className="size-4" aria-hidden /> Print
          </button>
          <button type="button" className={buttonClass("primary")} onClick={() => void savePdf()} disabled={!ready || saving}>
            {saving ? <Loader2 className="size-4 animate-spin" aria-hidden /> : <FileDown className="size-4" aria-hidden />}
            Save as PDF
          </button>
        </div>
      </div>

      {notice && (
        <p role="status" className="rounded-lg bg-teal-50 px-4 py-3 text-sm text-teal-900 ring-1 ring-teal-200 print:hidden">
          {notice}
        </p>
      )}
      {error && (
        <div className="print:hidden">
          <ErrorNotice>{error}</ErrorNotice>
        </div>
      )}

      {/* the report itself (printed) */}
      <article className="space-y-5" aria-busy={loading}>
        <header className="border-b border-slate-200 pb-4">
          <p className="text-xs font-semibold tracking-wide text-teal-800 uppercase">Clinilog · Clinic report</p>
          <h2 className="mt-1 text-2xl font-semibold tracking-tight">{session?.school.name}</h2>
          <p className="mt-1 text-sm text-slate-600">
            {periodLabel}
            {basic && (
              <span className="text-slate-400">
                {" "}
                · {formatDate(basic.period.start)} – {formatDate(basic.period.end)}
              </span>
            )}
          </p>
          <p className="mt-1 text-xs text-slate-500">
            Prepared {formatDateTime(generatedAt)} by {session?.user.full_name}
          </p>
        </header>

        {loading && !basic && (
          <p className="flex items-center gap-2 text-sm text-slate-500">
            <Loader2 className="size-4 animate-spin" aria-hidden /> Loading report…
          </p>
        )}

        {basic && (
          <>
            <div className="grid grid-cols-3 gap-4">
              <Stat label={`Visits · ${periodLabel}`} value={basic.total_visits} />
              <Stat label="Students seen" value={basic.students_seen} />
              <Stat
                label="Most common complaint"
                value={basic.top_complaints[0] ? capitalize(basic.top_complaints[0].term) : "—"}
                small
              />
            </div>

            <Section title={`Visits per month · ${year}`}>
              <MonthlyChart data={basic.visits_per_month} year={year} selectedMonth={month === "" ? undefined : month} />
            </Section>

            <Section
              title={`Most common complaints · ${periodLabel}`}
              note="Counted by the words used in each visit's complaint; a visit mentioning several counts toward each. Words used in only one visit aren't listed."
            >
              {basic.top_complaints.length === 0 ? (
                <Empty>
                  {basic.total_visits === 0 ? "No visits in this period." : "No complaint was mentioned in more than one visit."}
                </Empty>
              ) : (
                <ComplaintsTable rows={basic.top_complaints} total={basic.total_visits} />
              )}
            </Section>
          </>
        )}

        {/* detailed reports */}
        {features.detailed_reports ? (
          detailed && <DetailedSections report={detailed} periodLabel={periodLabel} />
        ) : (
          <div className="print:hidden">
            <PremiumGate feature="detailed_reports" label="Detailed reports">
              <Section title="Detailed reports" action={<PremiumBadge />}>
                <ul className="space-y-2 text-sm text-slate-600">
                  <li>Visits by grade and section</li>
                  <li>Medicine usage — quantities given per medicine</li>
                  <li>Frequent visitors — students with the most visits</li>
                </ul>
              </Section>
            </PremiumGate>
          </div>
        )}
      </article>
    </div>
  );
}

function DetailedSections({ report, periodLabel }: { report: DetailedReport; periodLabel: string }) {
  return (
    <>
      <Section title={`Visits by grade and section · ${periodLabel}`}>
        {report.by_section.length === 0 ? (
          <Empty>No visits in this period.</Empty>
        ) : (
          <Table head={["Grade", "Section", "Visits", "Students"]} align={["l", "l", "r", "r"]}>
            {report.by_section.map((r) => (
              <tr key={`${r.grade_level}-${r.section_name}`}>
                <Td>Grade {r.grade_level}</Td>
                <Td>{r.section_name}</Td>
                <Td right>{r.visits}</Td>
                <Td right>{r.students}</Td>
              </tr>
            ))}
          </Table>
        )}
      </Section>

      <Section title={`Medicine usage · ${periodLabel}`}>
        {report.medicines.length === 0 ? (
          <Empty>No medicines given in this period.</Empty>
        ) : (
          <Table head={["Medicine", "Total given", "Visits"]} align={["l", "r", "r"]}>
            {report.medicines.map((m) => (
              <tr key={m.name}>
                <Td>{m.name}</Td>
                <Td right>
                  {m.total_quantity} {m.unit}
                </Td>
                <Td right>{m.visits}</Td>
              </tr>
            ))}
          </Table>
        )}
      </Section>

      <Section title={`Frequent visitors · ${periodLabel}`} note="Up to 10 students with the most clinic visits in this period.">
        {report.frequent_visitors.length === 0 ? (
          <Empty>No visits in this period.</Empty>
        ) : (
          <Table head={["Student", "LRN", "Grade & section", "Visits", "Last visit"]} align={["l", "l", "l", "r", "l"]}>
            {report.frequent_visitors.map((f) => (
              <tr key={f.student_id}>
                <Td>
                  <Link to={`/students/${f.student_id}`} className="font-medium hover:underline">
                    {listName(f)}
                  </Link>
                </Td>
                <Td mono>{f.lrn}</Td>
                <Td>
                  Grade {f.grade_level} – {f.section_name}
                </Td>
                <Td right>{f.visits}</Td>
                <Td>{formatDateTime(f.last_visit)}</Td>
              </tr>
            ))}
          </Table>
        )}
      </Section>
    </>
  );
}

function ComplaintsTable({ rows, total }: { rows: { term: string; visits: number }[]; total: number }) {
  const max = Math.max(...rows.map((r) => r.visits));
  return (
    <table className="w-full text-sm">
      <thead className="text-xs text-slate-500">
        <tr>
          <th scope="col" className="w-8 pb-2 text-left font-medium">#</th>
          <th scope="col" className="pb-2 text-left font-medium">Complaint</th>
          <th scope="col" className="w-2/5 pb-2 font-medium">
            <span className="sr-only">Share</span>
          </th>
          <th scope="col" className="w-16 pb-2 text-right font-medium">Visits</th>
          <th scope="col" className="w-20 pb-2 text-right font-medium">% of visits</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r, i) => (
          <tr key={r.term} className="break-inside-avoid">
            <td className="py-1.5 text-slate-400 tabular-nums">{i + 1}</td>
            <td className="py-1.5 font-medium">{capitalize(r.term)}</td>
            <td className="py-1.5 pr-3" aria-hidden>
              <div className="h-2.5 rounded-r bg-teal-600" style={{ width: `${(r.visits / max) * 100}%` }} />
            </td>
            <td className="py-1.5 text-right tabular-nums">{r.visits}</td>
            <td className="py-1.5 text-right text-slate-600 tabular-nums">{total ? Math.round((r.visits / total) * 100) : 0}%</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

function Section({ title, note, action, children }: { title: string; note?: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="break-inside-avoid rounded-xl border border-slate-200 bg-white p-5 shadow-sm print:shadow-none">
      <div className="mb-3 flex items-center justify-between gap-3">
        <h3 className="text-sm font-semibold text-slate-800">{title}</h3>
        {action}
      </div>
      {note && <p className="-mt-2 mb-3 text-xs text-slate-500">{note}</p>}
      {children}
    </section>
  );
}

function Stat({ label, value, small }: { label: string; value: ReactNode; small?: boolean }) {
  return (
    <div className="rounded-xl border border-slate-200 bg-white p-4 shadow-sm print:shadow-none">
      <p className="text-xs font-medium text-slate-500">{label}</p>
      <p className={`mt-1 font-semibold tabular-nums ${small ? "truncate text-lg" : "text-3xl"}`}>{value}</p>
    </div>
  );
}

function Table({ head, align, children }: { head: string[]; align: ("l" | "r")[]; children: ReactNode }) {
  return (
    <table className="w-full text-sm">
      <thead className="border-b border-slate-200 text-xs text-slate-500">
        <tr>
          {head.map((h, i) => (
            <th key={h} scope="col" className={`pr-4 pb-2 font-medium last:pr-0 ${align[i] === "r" ? "text-right" : "text-left"}`}>
              {h}
            </th>
          ))}
        </tr>
      </thead>
      <tbody className="divide-y divide-slate-100">{children}</tbody>
    </table>
  );
}

function Td({ children, right, mono }: { children: ReactNode; right?: boolean; mono?: boolean }) {
  return (
    <td className={`py-2 pr-4 last:pr-0 ${right ? "text-right tabular-nums" : ""} ${mono ? "font-mono text-slate-600 tabular-nums" : ""}`}>
      {children}
    </td>
  );
}

function Empty({ children }: { children: ReactNode }) {
  return <p className="py-4 text-sm text-slate-500">{children}</p>;
}

function capitalize(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
