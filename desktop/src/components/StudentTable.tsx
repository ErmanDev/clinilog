import { Link, useNavigate } from "react-router-dom";
import type { Student } from "../lib/api";
import { genderLabel, listName, sectionLabel } from "../lib/format";

/** Students table shared by the Students page and Search. Rows open the student. */
export function StudentTable({ students }: { students: Student[] }) {
  const navigate = useNavigate();
  if (students.length === 0) return null;
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-left text-sm">
        <thead className="border-b border-slate-200 bg-slate-50 text-xs font-semibold tracking-wide text-slate-500 uppercase">
          <tr>
            <th scope="col" className="px-4 py-3">Name</th>
            <th scope="col" className="px-4 py-3">LRN</th>
            <th scope="col" className="px-4 py-3">Grade &amp; section</th>
            <th scope="col" className="px-4 py-3">Gender</th>
            <th scope="col" className="px-4 py-3 text-right">Age</th>
          </tr>
        </thead>
        <tbody className="divide-y divide-slate-100">
          {students.map((s) => (
            <tr key={s.id} onClick={() => navigate(`/students/${s.id}`)} className="cursor-pointer hover:bg-teal-50/50">
              <td className="px-4 py-3 font-medium">
                <Link
                  to={`/students/${s.id}`}
                  className="outline-none hover:underline focus-visible:underline"
                  onClick={(e) => e.stopPropagation()}
                >
                  {listName(s)}
                </Link>
              </td>
              <td className="px-4 py-3 font-mono text-slate-600 tabular-nums">{s.lrn}</td>
              <td className="px-4 py-3 text-slate-600">{sectionLabel(s.section)}</td>
              <td className="px-4 py-3 text-slate-600">{genderLabel(s.gender)}</td>
              <td className="px-4 py-3 text-right text-slate-600 tabular-nums">{s.age}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
