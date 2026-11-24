// Thin typed wrappers over the Tauri commands in src-tauri/src/lib.rs.
// The webview never talks to the server directly and never sees the token.
import { invoke } from "@tauri-apps/api/core";

/** "superadmin" is the one Clinilog-wide account that reviews payments; it never appears in User Accounts. */
export type Role = "admin" | "staff" | "superadmin";
export type AccountRole = "admin" | "staff";
export type PlanName = "basic" | "premium";

export interface UserInfo {
  id: string;
  full_name: string;
  username: string;
  role: Role;
}

export interface SchoolInfo {
  id: string;
  name: string;
}

export interface SessionInfo {
  user: UserInfo;
  school: SchoolInfo;
  /** Logged in without the server (offline login). */
  offline: boolean;
}

export interface Features {
  advanced_search: boolean;
  detailed_reports: boolean;
  daily_backups: boolean;
}

export interface SubscriptionInfo {
  plan: PlanName;
  status: "active" | "expired" | "cancelled" | "none";
  start_date: string | null;
  end_date: string | null;
  days_left: number | null;
  read_only: boolean;
  limits: { max_students: number | null; max_users: number };
  features: Features;
  usage: { students: number; active_users: number };
  server_date: string;
}

export interface PlanState {
  info: SubscriptionInfo;
  source: "server" | "cache";
  fetched_at: string;
  grace_ends_at: string | null;
  grace_expired: boolean;
}

export interface ConfigInfo {
  api_url: string;
  school_configured: boolean;
  problem: string | null;
  version: string;
}

export type ErrorKind =
  | "network"
  | "unauthorized"
  | "bad_credentials"
  | "locked"
  | "forbidden"
  | "invalid"
  | "not_found"
  | "conflict"
  | "config"
  | "server";

export interface CmdError {
  kind: ErrorKind;
  message: string;
  retry_after_secs?: number;
  /** Per-field messages on a 400, keyed by field name. */
  fields?: Record<string, string>;
  /** On a 409: which conflict, e.g. "possible_duplicate". */
  code?: string;
  details?: unknown;
}

function toCmdError(e: unknown): CmdError {
  if (e && typeof e === "object" && "kind" in e && "message" in e) return e as CmdError;
  return { kind: "server", message: typeof e === "string" ? e : "Something went wrong." };
}

let onUnauthorized: ((message: string) => void) | null = null;

/** The session provider registers this so an expired session anywhere returns to login. */
export function setUnauthorizedHandler(fn: ((message: string) => void) | null) {
  onUnauthorized = fn;
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    const err = toCmdError(e);
    if (err.kind === "unauthorized" && cmd !== "login") onUnauthorized?.(err.message);
    throw err;
  }
}

type Method = "GET" | "POST" | "PUT" | "DELETE";

function request<T>(method: Method, path: string, body?: unknown): Promise<T> {
  return call<T>("api_request", { method, path, body: body ?? null });
}

function qs(params: Record<string, string | number | null | undefined>): string {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) {
    if (v !== null && v !== undefined && v !== "") p.set(k, String(v));
  }
  const s = p.toString();
  return s ? `?${s}` : "";
}

// ---------- Sections & students ----------

export type Gender = "male" | "female";

export interface Section {
  id: string;
  grade_level: number;
  section_name: string;
  student_count: number;
}

export interface SectionInput {
  grade_level: number;
  section_name: string;
}

export interface Student {
  id: string;
  lrn: string;
  first_name: string;
  middle_name: string | null;
  last_name: string;
  gender: Gender;
  birthdate: string;
  age: number;
  section: { id: string; grade_level: number; section_name: string };
  created_at: string;
  updated_at: string;
}

export interface VisitSummary {
  id: string;
  visited_at: string;
  complaint: string;
  treatment: string;
  attended_by: string;
  medicines: { name: string; unit: string; quantity: number }[];
}

export interface StudentDetail extends Student {
  visits: VisitSummary[];
}

export interface StudentPage {
  items: Student[];
  total: number;
  page: number;
  page_size: number;
}

export interface StudentInput {
  id?: string;
  lrn: string;
  first_name: string;
  middle_name: string;
  last_name: string;
  gender: Gender | "";
  birthdate: string;
  section_id: string | null;
  allow_duplicate?: boolean;
}

export interface DuplicateMatch {
  id: string;
  lrn: string;
  first_name: string;
  last_name: string;
  grade_level: number;
  section_name: string;
}

// ---------- Medicines & visits ----------

export interface Medicine {
  id: string;
  name: string;
  unit: string;
  visit_count: number;
}

export interface MedicineInput {
  name: string;
  unit: string;
}

export interface VisitMedicine {
  medicine_id: string;
  name: string;
  unit: string;
  quantity: number;
}

export interface Visit {
  id: string;
  visited_at: string;
  complaint: string;
  treatment: string;
  created_at: string;
  student: {
    id: string;
    lrn: string;
    first_name: string;
    middle_name: string | null;
    last_name: string;
    grade_level: number;
    section_name: string;
  };
  attended_by: { id: string; full_name: string };
  medicines: VisitMedicine[];
}

export interface VisitPage {
  items: Visit[];
  total: number;
  page: number;
  page_size: number;
}

export interface VisitInput {
  id?: string;
  student_id: string | null;
  complaint: string;
  treatment: string;
  /** quantity is sent as typed so the server can explain bad values. */
  medicines: { medicine_id: string | null; quantity: number | string }[];
}

export interface VisitSearch {
  q?: string;
  section_id?: string;
  grade_level?: number;
  date_from?: string;
  date_to?: string;
  complaint?: string;
}

export interface Dashboard {
  today: string;
  today_count: number;
  recent: Visit[];
}

export const medicinesApi = {
  list: () => request<Medicine[]>("GET", "medicines"),
  create: (input: MedicineInput) => request<Medicine>("POST", "medicines", input),
  update: (id: string, input: MedicineInput) => request<Medicine>("PUT", `medicines/${encodeURIComponent(id)}`, input),
  remove: (id: string) => request<null>("DELETE", `medicines/${encodeURIComponent(id)}`),
};

export const visitsApi = {
  /** `grade_level`, `date_from`, `date_to` and `complaint` are advanced (Premium) filters. */
  list: (params: VisitSearch & { date?: string; student_id?: string; page?: number; page_size?: number }) =>
    request<VisitPage>("GET", `visits${qs({ ...params })}`),
  get: (id: string) => request<Visit>("GET", `visits/${encodeURIComponent(id)}`),
  create: (input: VisitInput) => request<Visit>("POST", "visits", input),
  update: (id: string, input: VisitInput) => request<Visit>("PUT", `visits/${encodeURIComponent(id)}`, input),
  remove: (id: string) => request<null>("DELETE", `visits/${encodeURIComponent(id)}`),
  dashboard: () => request<Dashboard>("GET", "dashboard"),
};

// ---------- Reports ----------

export interface ReportPeriod {
  year: number;
  month: number | null;
  start: string;
  end: string;
}

export interface BasicReport {
  period: ReportPeriod;
  total_visits: number;
  students_seen: number;
  visits_per_month: { month: number; visits: number }[];
  top_complaints: { term: string; visits: number }[];
}

export interface DetailedReport {
  period: ReportPeriod;
  by_section: { grade_level: number; section_name: string; visits: number; students: number }[];
  medicines: { name: string; unit: string; total_quantity: number; visits: number }[];
  frequent_visitors: {
    student_id: string;
    lrn: string;
    first_name: string;
    middle_name: string | null;
    last_name: string;
    grade_level: number;
    section_name: string;
    visits: number;
    last_visit: string;
  }[];
}

export const reportsApi = {
  basic: (year: number, month?: number) => request<BasicReport>("GET", `reports/basic${qs({ year, month })}`),
  /** Premium only. */
  detailed: (year: number, month?: number) => request<DetailedReport>("GET", `reports/detailed${qs({ year, month })}`),
  /** Opens the Save dialog and writes the current page (print layout) as a PDF. Null if cancelled. */
  savePdf: (fileName: string) => call<string | null>("save_report_pdf", { fileName }),
};

// ---------- Subscription payments (GCash, reviewed by the superadmin) ----------

export type Term = "monthly" | "yearly";

export interface PlanOption {
  plan: PlanName;
  monthly_php: number;
  yearly_php: number;
  limits: { max_students: number | null; max_users: number };
  features: Features;
}

export interface PaymentInfo {
  method: "gcash";
  label: string;
  account_name: string;
  /** data:image/png;base64,… */
  qr_image: string;
}

export interface PlansResponse {
  plans: PlanOption[];
  payment: PaymentInfo;
}

export type RequestStatus = "pending" | "approved" | "rejected" | "cancelled";

export interface PaymentRequest {
  id: string;
  plan: PlanName;
  term: Term;
  amount_php: number;
  payment_method: string;
  reference_no: string;
  status: RequestStatus;
  created_at: string;
  reviewed_at: string | null;
  /** Reason for a rejection (or the superadmin's note on an approval). */
  review_note: string | null;
  requested_by_name: string;
  /** The plan period, once approved. */
  start_date: string | null;
  end_date: string | null;
}

export const subscriptionApi = {
  plans: () => request<PlansResponse>("GET", "subscription/plans"),
  requests: () => request<PaymentRequest[]>("GET", "subscription/requests"),
  /** `receipt_image` is a data URL. The plan starts only after the superadmin approves. */
  submit: (input: { plan: PlanName; term: Term; reference_no: string; receipt_image: string }) =>
    request<PaymentRequest>("POST", "subscription/requests", input),
  cancel: (id: string) => request<PaymentRequest>("POST", `subscription/requests/${encodeURIComponent(id)}/cancel`),
};

// ---------- Superadmin: payment review ----------

export interface ReviewItem extends Omit<PaymentRequest, "requested_by_name"> {
  school_id: string;
  school_name: string;
  requested_by_name: string;
  requested_by_username: string;
}

export interface ReviewDetail extends ReviewItem {
  receipt_image: string;
  current_subscription: SubscriptionInfo;
  /** Other requests (any school, any status) that used the same reference number. */
  reference_uses_elsewhere: number;
}

export interface SchoolSummary {
  id: string;
  name: string;
  plan: PlanName | null;
  status: "active" | "expired" | "cancelled" | null;
  end_date: string | null;
  pending_requests: number;
  last_paid_at: string | null;
}

export const superadminApi = {
  requests: (status: RequestStatus | "all") => request<ReviewItem[]>("GET", `superadmin/requests${qs({ status })}`),
  request: (id: string) => request<ReviewDetail>("GET", `superadmin/requests/${encodeURIComponent(id)}`),
  approve: (id: string, note?: string) =>
    request<ReviewItem>("POST", `superadmin/requests/${encodeURIComponent(id)}/approve`, { note: note || null }),
  reject: (id: string, note: string) => request<ReviewItem>("POST", `superadmin/requests/${encodeURIComponent(id)}/reject`, { note }),
  schools: () => request<SchoolSummary[]>("GET", "superadmin/schools"),
};

// ---------- Admin: users & activity ----------

export interface Account {
  id: string;
  full_name: string;
  username: string;
  role: AccountRole;
  is_active: boolean;
  created_at: string;
}

export interface ActivityEntry {
  id: string;
  created_at: string;
  action: "view" | "create" | "update" | "delete";
  table_name: "students" | "clinic_visits" | "users" | "subscriptions" | "subscription_requests";
  record_id: string;
  record_label: string | null;
  user_id: string;
  user_full_name: string;
  user_username: string;
}

export interface ActivityPage {
  items: ActivityEntry[];
  total: number;
  page: number;
  page_size: number;
}

export const usersApi = {
  list: () => request<Account[]>("GET", "users"),
  create: (input: { full_name: string; username: string; password: string; role: AccountRole }) =>
    request<Account>("POST", "users", input),
  update: (id: string, input: { full_name: string; role: AccountRole }) =>
    request<Account>("PUT", `users/${encodeURIComponent(id)}`, input),
  disable: (id: string) => request<Account>("POST", `users/${encodeURIComponent(id)}/disable`),
  enable: (id: string) => request<Account>("POST", `users/${encodeURIComponent(id)}/enable`),
  resetPassword: (id: string, password: string) =>
    request<null>("POST", `users/${encodeURIComponent(id)}/reset-password`, { password }),
};

export const activityApi = {
  list: (params: {
    user_id?: string;
    action?: string;
    table?: string;
    date_from?: string;
    date_to?: string;
    page?: number;
    page_size?: number;
  }) => request<ActivityPage>("GET", `activity${qs(params)}`),
};

// ---------- Offline cache & sync (src-tauri/src/sync.rs) ----------

export interface SyncStatus {
  online: boolean;
  session_offline: boolean;
  pending: number;
  failed: number;
  others_waiting: number;
  lists_refreshed_at: string | null;
  auth_required: boolean;
  cache_error: string | null;
}

export interface CachedStudent {
  id: string;
  lrn: string;
  first_name: string;
  middle_name: string | null;
  last_name: string;
  grade_level: number;
  section_name: string;
}

export interface OutboxEntry {
  id: string;
  mine: boolean;
  status: "pending" | "failed";
  attempts: number;
  last_error: string | null;
  visited_at: string;
  student_label: string;
  complaint: string;
  recorded_by: string;
}

export interface VisitDraft {
  student_id: string;
  complaint: string;
  treatment: string;
  medicines: { medicine_id: string; quantity: number }[];
  student_label: string;
}

export const syncApi = {
  status: () => call<SyncStatus>("sync_status"),
  syncNow: () => call<SyncStatus>("sync_now"),
  /** Saves on this PC first, then uploads if online. */
  recordVisit: (draft: VisitDraft) => call<{ status: "synced" | "pending"; visit_id: string }>("record_visit", { draft }),
  outbox: () => call<OutboxEntry[]>("outbox_list"),
  retry: (id: string) => call<void>("outbox_retry", { id }),
  discard: (id: string) => call<void>("outbox_discard", { id }),
  cachedStudents: (q: string) => call<CachedStudent[]>("cached_students", { q }),
  cachedStudent: (id: string) => call<CachedStudent | null>("cached_student", { id }),
  cachedMedicines: () => call<{ id: string; name: string; unit: string }[]>("cached_medicines"),
};

export const sectionsApi = {
  list: () => request<Section[]>("GET", "sections"),
  create: (input: SectionInput) => request<Section>("POST", "sections", input),
  update: (id: string, input: SectionInput) => request<Section>("PUT", `sections/${encodeURIComponent(id)}`, input),
  remove: (id: string) => request<null>("DELETE", `sections/${encodeURIComponent(id)}`),
};

export const studentsApi = {
  /** `grade_level` is an advanced (Premium) filter. */
  list: (params: { q?: string; section_id?: string | null; grade_level?: number; page?: number; page_size?: number }) =>
    request<StudentPage>("GET", `students${qs(params)}`),
  get: (id: string) => request<StudentDetail>("GET", `students/${encodeURIComponent(id)}`),
  /** Is this LRN free in the school? `exclude` = the student being edited. */
  lrnCheck: (lrn: string, exclude?: string) =>
    request<{ available: boolean; student: { id: string; first_name: string; last_name: string } | null }>(
      "GET",
      `students/lrn-check${qs({ lrn, exclude })}`,
    ),
  create: (input: StudentInput) => request<Student>("POST", "students", input),
  update: (id: string, input: StudentInput) => request<Student>("PUT", `students/${encodeURIComponent(id)}`, input),
  remove: (id: string) => request<null>("DELETE", `students/${encodeURIComponent(id)}`),
};

export const api = {
  appConfig: () => call<ConfigInfo>("app_config"),
  checkServer: () => call<void>("check_server"),
  login: (username: string, password: string) => call<SessionInfo>("login", { username, password }),
  logout: () => call<void>("logout"),
  currentSession: () => call<SessionInfo | null>("current_session"),
  fetchPlan: () => call<PlanState>("fetch_plan"),
};

export function asCmdError(e: unknown): CmdError {
  return toCmdError(e);
}
