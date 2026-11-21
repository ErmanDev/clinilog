const dateFmt = new Intl.DateTimeFormat("en-PH", { year: "numeric", month: "long", day: "numeric" });

/** Formats an ISO date ("2027-11-19") or timestamp as "November 19, 2027". */
export function formatDate(iso: string): string {
  // Plain dates are parsed as local midnight so they don't shift a day across time zones.
  const d = /^\d{4}-\d{2}-\d{2}$/.test(iso) ? new Date(`${iso}T00:00:00`) : new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : dateFmt.format(d);
}

const dateTimeFmt = new Intl.DateTimeFormat("en-PH", {
  timeZone: "Asia/Manila",
  year: "numeric",
  month: "short",
  day: "numeric",
  hour: "numeric",
  minute: "2-digit",
});

/** A timestamp in Philippine time, e.g. "Nov 20, 2026, 9:05 AM". */
export function formatDateTime(iso: string): string {
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? iso : dateTimeFmt.format(d);
}

interface NameParts {
  first_name: string;
  middle_name: string | null;
  last_name: string;
}

/** "Juan S. Dela Cruz" */
export function fullName(s: NameParts): string {
  return [s.first_name, s.middle_name ? `${[...s.middle_name][0]}.` : null, s.last_name].filter(Boolean).join(" ");
}

/** "Dela Cruz, Juan S." — for sorted lists. */
export function listName(s: NameParts): string {
  return `${s.last_name}, ${s.first_name}${s.middle_name ? ` ${[...s.middle_name][0]}.` : ""}`;
}

export function sectionLabel(s: { grade_level: number; section_name: string }): string {
  return `Grade ${s.grade_level} – ${s.section_name}`;
}

export function genderLabel(g: string): string {
  return g === "female" ? "Female" : g === "male" ? "Male" : g;
}

export function planLabel(plan: string): string {
  return plan === "premium" ? "Premium" : "Basic";
}
