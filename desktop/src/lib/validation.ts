// Same rules as server/src/validate.rs (spec §7a). These give instant feedback;
// the server checks everything again and has the final say.

export const NAME_MAX = 50;
export const MIN_AGE = 4;
export const MAX_AGE = 25;

const NAME_RE = /^[\p{L}\p{M} '’.\-]+$/u;

export function validateName(value: string, label: string, required = true): string | null {
  const v = value.trim();
  if (!v) return required ? `Enter the ${label}.` : null;
  if ([...v].length > NAME_MAX) return `${cap(label)} must be ${NAME_MAX} characters or fewer.`;
  if (!NAME_RE.test(v) || !/\p{L}/u.test(v)) {
    return `${cap(label)} can only have letters, spaces, hyphens (-), apostrophes (') and periods (.).`;
  }
  return null;
}

export function validateLrn(value: string): string | null {
  const v = value.trim();
  if (!v) return "Enter the LRN.";
  if (!/^[0-9]{12}$/.test(v)) return "LRN must be exactly 12 digits.";
  return null;
}

export function validateGender(value: string): string | null {
  return value === "male" || value === "female" ? null : "Choose a gender.";
}

/** Today's date in the Philippines as YYYY-MM-DD, whatever the PC's time zone. */
export function todayPH(): string {
  return new Intl.DateTimeFormat("en-CA", { timeZone: "Asia/Manila" }).format(new Date());
}

export function ageOn(birthdate: string, on: string): number {
  const [by, bm, bd] = birthdate.split("-").map(Number);
  const [ty, tm, td] = on.split("-").map(Number);
  let age = ty - by;
  if (tm < bm || (tm === bm && td < bd)) age -= 1;
  return age;
}

function isRealDate(v: string): boolean {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(v)) return false;
  const [y, m, d] = v.split("-").map(Number);
  const dt = new Date(Date.UTC(y, m - 1, d));
  return dt.getUTCFullYear() === y && dt.getUTCMonth() === m - 1 && dt.getUTCDate() === d;
}

export function validateBirthdate(value: string): string | null {
  const v = value.trim();
  if (!v) return "Enter the birthdate.";
  if (!isRealDate(v)) return "Enter a valid date.";
  const today = todayPH();
  if (v > today) return "Birthdate can't be in the future.";
  const age = ageOn(v, today);
  if (age < MIN_AGE || age > MAX_AGE) return `Age must be between ${MIN_AGE} and ${MAX_AGE} (this birthdate gives ${age}).`;
  return null;
}

/** Earliest and latest birthdates that give an age of 4–25 today, for the date picker. */
export function birthdateBounds(): { min: string; max: string } {
  const [y, m, d] = todayPH().split("-").map(Number);
  const iso = (yy: number, mm: number, dd: number) => {
    const dt = new Date(Date.UTC(yy, mm - 1, dd));
    return dt.toISOString().slice(0, 10);
  };
  return { max: iso(y - MIN_AGE, m, d), min: iso(y - MAX_AGE - 1, m, d + 1) };
}

export function validateSectionName(value: string): string | null {
  const v = value.trim();
  if (!v) return "Enter the section name.";
  if ([...v].length > 50) return "Section name must be 50 characters or fewer.";
  return null;
}

/** Required text with a length range after trimming (complaint 3–500, treatment 1–500). */
export function validateText(value: string, label: string, min: number, max: number): string | null {
  const v = value.trim();
  const len = [...v].length;
  if (len === 0) return `Enter the ${label}.`;
  if (len < min) return `${cap(label)} must be at least ${min} characters.`;
  if (len > max) return `${cap(label)} must be ${max} characters or fewer.`;
  return null;
}

/** Medicine quantity: whole number 1–100. Takes the raw input text. */
export function validateQuantity(value: string): string | null {
  const v = value.trim();
  if (!v) return "Enter a quantity.";
  if (!/^\d+$/.test(v)) return "Quantity must be a whole number.";
  const n = Number(v);
  if (n < 1 || n > 100) return "Quantity must be from 1 to 100.";
  return null;
}

export function validateShortText(value: string, label: string, max: number): string | null {
  const v = value.trim();
  if (!v) return `Enter the ${label}.`;
  if ([...v].length > max) return `Must be ${max} characters or fewer.`;
  return null;
}

export function validateUsername(value: string): string | null {
  const v = value.trim();
  if (!v) return "Enter a username.";
  if (!/^[A-Za-z0-9_]{4,30}$/.test(v)) return "Username must be 4–30 characters: letters, numbers or underscore (_).";
  return null;
}

/** At least 8 characters with a letter and a number (not trimmed; case-sensitive). */
export function validatePassword(value: string): string | null {
  if ([...value].length < 8 || !/\p{L}/u.test(value) || !/[0-9]/.test(value)) {
    return "Password must be at least 8 characters, with at least one letter and one number.";
  }
  if (value.length > 128) return "Password must be 128 characters or fewer.";
  return null;
}

export function validateFullName(value: string): string | null {
  const v = value.trim();
  if (!v) return "Enter the full name.";
  if ([...v].length > 100) return "Full name must be 100 characters or fewer.";
  if (!NAME_RE.test(v) || !/\p{L}/u.test(v)) {
    return "Full name can only have letters, spaces, hyphens (-), apostrophes (') and periods (.).";
  }
  return null;
}

function cap(s: string) {
  return s.charAt(0).toUpperCase() + s.slice(1);
}
