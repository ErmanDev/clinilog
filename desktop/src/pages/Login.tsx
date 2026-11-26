import { AlertCircle, ArrowLeft, Building2, Eye, EyeOff, Loader2, RefreshCw, WifiOff } from "lucide-react";
import { useCallback, useEffect, useId, useState, type FormEvent } from "react";
import { api, asCmdError, type ConfigInfo, type SchoolRef } from "../lib/api";
import { useSession } from "../lib/session";
import { WindowControls } from "../components/WindowControls";
import logoMark from "../assets/clinilog-mark.png";
import { ClinicScene } from "../components/ClinicScene";

/** How many times to ping the server before giving up (each try waits up to 20 s). */
const MAX_CONNECT_TRIES = 4;
/** While offline, check quietly this often whether the connection is back. */
const OFFLINE_RECHECK_MS = 10_000;
/** A quick check never shows "Connecting…"; only one that takes longer than this. */
const SHOW_CHECKING_AFTER_MS = 1500;

type Conn = { state: "checking"; attempt: number } | { state: "online" } | { state: "offline"; message: string };

export function LoginPage() {
  const { signIn, endedReason } = useSession();
  const [config, setConfig] = useState<ConfigInfo | null>(null);
  const [conn, setConn] = useState<Conn>({ state: "checking", attempt: 1 });

  const [username, setUsername] = useState("");
  const [password, setPassword] = useState("");
  const [touched, setTouched] = useState({ username: false, password: false });
  const [showPassword, setShowPassword] = useState(false);
  const [capsLock, setCapsLock] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [formError, setFormError] = useState<string | null>(null);
  const [lockedUntil, setLockedUntil] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  /** Signing in as the Clinilog superadmin on a PC with no school set. */
  const [superadminMode, setSuperadminMode] = useState(false);

  const userId = useId();
  const passId = useId();

  // Wake the server (Render's free tier sleeps) before letting staff sign in.
  // No internet at all shows "offline" straight away: nothing to wait for.
  const connect = useCallback(async () => {
    for (let attempt = 1; attempt <= MAX_CONNECT_TRIES; attempt++) {
      setConn({ state: "checking", attempt });
      try {
        await api.checkServer();
        setConn({ state: "online" });
        return;
      } catch (e) {
        const err = asCmdError(e);
        if (err.kind !== "network" || err.code === "offline" || attempt === MAX_CONNECT_TRIES) {
          setConn({ state: "offline", message: err.message });
          return;
        }
        await new Promise((r) => setTimeout(r, 2000));
      }
    }
  }, []);

  // Offline: keep checking in the background and clear the notice once the internet is back.
  useEffect(() => {
    if (conn.state !== "offline") return;
    const id = window.setInterval(() => {
      api.checkServer().then(
        () => setConn({ state: "online" }),
        () => {},
      );
    }, OFFLINE_RECHECK_MS);
    return () => window.clearInterval(id);
  }, [conn.state]);

  useEffect(() => {
    api.appConfig().then((c) => {
      setConfig(c);
      if (!c.problem) void connect();
    });
  }, [connect]);

  // Tick once a second while locked so the countdown updates.
  useEffect(() => {
    if (!lockedUntil) return;
    const id = window.setInterval(() => {
      setNow(Date.now());
      if (Date.now() >= lockedUntil) setLockedUntil(null);
    }, 1000);
    return () => window.clearInterval(id);
  }, [lockedUntil]);

  const usernameError = username.trim() === "" ? "Enter your username." : null;
  const passwordError = password === "" ? "Enter your password." : null;
  const lockedSecs = lockedUntil ? Math.max(0, Math.ceil((lockedUntil - now) / 1000)) : 0;
  const canSubmit = conn.state !== "checking" && !usernameError && !passwordError && !submitting && lockedSecs === 0;

  async function onSubmit(e: FormEvent) {
    e.preventDefault();
    setTouched({ username: true, password: true });
    if (!canSubmit) return;
    setSubmitting(true);
    setFormError(null);
    try {
      await signIn(username, password);
    } catch (err) {
      const ce = asCmdError(err);
      if (ce.kind === "locked" && ce.retry_after_secs) {
        setNow(Date.now());
        setLockedUntil(Date.now() + ce.retry_after_secs * 1000);
      } else if (ce.kind === "network") {
        setConn({ state: "offline", message: ce.message });
        setFormError(ce.message);
      } else {
        setFormError(ce.message);
      }
      setPassword("");
    } finally {
      setSubmitting(false);
    }
  }

  const onKey = (e: React.KeyboardEvent) => setCapsLock(e.getModifierState("CapsLock"));

  async function changeSchool() {
    setFormError(null);
    try {
      await api.clearSchool();
      setConfig((c) => (c ? { ...c, school: null, school_configured: false } : c));
    } catch (err) {
      setFormError(asCmdError(err).message);
    }
  }

  const school = config?.school ?? null;
  const needsSchool = !!config && !config.problem && !school && !superadminMode;

  return (
    <div className="relative flex h-full">
      <div className="absolute top-3 right-3 z-10">
        <WindowControls />
      </div>
      <section className="relative hidden w-[46%] overflow-hidden bg-gradient-to-b from-teal-800 to-teal-900 text-white lg:block">
        {/* Decorative animation: students checking in and being treated, on a loop. */}
        <ClinicScene className="absolute inset-0" />
        <div className="pointer-events-none relative flex h-full flex-col justify-between p-10">
          <div className="flex items-center gap-3">
            <span className="flex size-11 items-center justify-center rounded-xl bg-white p-1.5" aria-hidden>
              <img src={logoMark} alt="" className="size-full" draggable={false} />
            </span>
            <span className="text-2xl font-bold tracking-tight">Clinilog</span>
          </div>
          <div>
            <p className="text-2xl font-semibold leading-snug">School clinic records, kept safe and simple.</p>
            <p className="mt-2 max-w-sm text-sm text-teal-100">
              Student health profiles, clinic visits and reports in one place.
            </p>
            <p className="mt-6 text-xs text-teal-200">Student health data is protected under RA 10173 (Data Privacy Act).</p>
          </div>
        </div>
      </section>

      <section className="flex flex-1 items-center justify-center p-8">
        <div className="w-full max-w-sm">
          <div className="mb-8 flex items-center gap-2.5 lg:hidden">
            <img src={logoMark} alt="" className="size-9" draggable={false} />
            <span className="text-2xl font-bold tracking-tight text-teal-800">Clinilog</span>
          </div>
          {needsSchool ? (
            <>
              <h1 className="text-2xl font-semibold">Connect to your school</h1>
              <p className="mt-1 text-sm text-slate-500">Enter your school code. You only need to do this once on this PC.</p>
            </>
          ) : (
            <>
              <h1 className="text-2xl font-semibold">{superadminMode && !school ? "Superadmin log in" : "Log in"}</h1>
              {superadminMode && !school ? (
                <button
                  type="button"
                  onClick={() => {
                    setSuperadminMode(false);
                    setFormError(null);
                  }}
                  className="mt-1 inline-flex items-center gap-1 text-sm font-medium text-teal-700 hover:underline"
                >
                  <ArrowLeft className="size-3.5" aria-hidden /> Back to school code
                </button>
              ) : (
                <p className="mt-1 text-sm text-slate-500">Use the account your clinic admin gave you.</p>
              )}
              {school?.name && (
                <div className="mt-4 flex items-center justify-between gap-3 rounded-lg bg-slate-100 px-3 py-2 text-sm">
                  <span className="flex min-w-0 items-center gap-2">
                    <Building2 className="size-4 shrink-0 text-slate-500" aria-hidden />
                    <span className="truncate font-medium" title={school.name}>
                      {school.name}
                    </span>
                    {school.code && <span className="shrink-0 font-mono text-xs text-slate-500">{school.code}</span>}
                  </span>
                  {!config?.school_locked && (
                    <button type="button" onClick={() => void changeSchool()} className="shrink-0 text-xs font-semibold text-teal-700 hover:underline">
                      Change school
                    </button>
                  )}
                </div>
              )}
            </>
          )}

          {endedReason && (
            <Notice tone="warn" icon={<AlertCircle className="size-4" />}>
              {endedReason}
            </Notice>
          )}
          {config?.problem ? (
            <Notice tone="danger" icon={<AlertCircle className="size-4" />}>
              {config.problem}
            </Notice>
          ) : (
            <ConnectionStatus conn={conn} onRetry={() => void connect()} />
          )}

          {needsSchool ? (
            <SchoolCodeForm
              disabled={conn.state === "checking"}
              onConnected={(s) => setConfig((c) => (c ? { ...c, school: s, school_configured: true } : c))}
              onSuperadmin={() => setSuperadminMode(true)}
            />
          ) : (
          <form onSubmit={onSubmit} noValidate className="mt-6 space-y-5">
            <div>
              <label htmlFor={userId} className="block text-sm font-medium text-slate-700">
                Username
              </label>
              <input
                id={userId}
                value={username}
                onChange={(e) => setUsername(e.target.value)}
                onBlur={() => setTouched((t) => ({ ...t, username: true }))}
                autoComplete="username"
                autoFocus
                maxLength={30}
                aria-invalid={touched.username && !!usernameError}
                aria-describedby={touched.username && usernameError ? `${userId}-err` : undefined}
                className={inputClass(touched.username && !!usernameError)}
              />
              {touched.username && usernameError && <FieldError id={`${userId}-err`}>{usernameError}</FieldError>}
            </div>

            <div>
              <label htmlFor={passId} className="block text-sm font-medium text-slate-700">
                Password
              </label>
              <div className="relative">
                <input
                  id={passId}
                  type={showPassword ? "text" : "password"}
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  onBlur={() => setTouched((t) => ({ ...t, password: true }))}
                  onKeyUp={onKey}
                  onKeyDown={onKey}
                  autoComplete="current-password"
                  aria-invalid={touched.password && !!passwordError}
                  aria-describedby={touched.password && passwordError ? `${passId}-err` : undefined}
                  className={`${inputClass(touched.password && !!passwordError)} pr-10`}
                />
                <button
                  type="button"
                  onClick={() => setShowPassword((s) => !s)}
                  className="absolute inset-y-0 right-0 mt-1 flex items-center px-3 text-slate-500 hover:text-slate-700"
                  aria-label={showPassword ? "Hide password" : "Show password"}
                >
                  {showPassword ? <EyeOff className="size-4" /> : <Eye className="size-4" />}
                </button>
              </div>
              {touched.password && passwordError && <FieldError id={`${passId}-err`}>{passwordError}</FieldError>}
              {capsLock && <p className="mt-1.5 text-xs font-medium text-amber-700">Caps Lock is on.</p>}
            </div>

            {lockedSecs > 0 && (
              <Notice tone="danger" icon={<AlertCircle className="size-4" />}>
                Too many failed attempts. Try again in {formatCountdown(lockedSecs)}.
              </Notice>
            )}
            {formError && lockedSecs === 0 && (
              <Notice tone="danger" icon={<AlertCircle className="size-4" />}>
                {formError}
              </Notice>
            )}

            <button
              type="submit"
              disabled={!canSubmit}
              className="flex w-full items-center justify-center gap-2 rounded-lg bg-teal-700 px-4 py-2.5 text-sm font-semibold text-white hover:bg-teal-800 disabled:cursor-not-allowed disabled:bg-slate-300"
            >
              {submitting && <Loader2 className="size-4 animate-spin" aria-hidden />}
              {submitting ? "Logging in…" : "Log in"}
            </button>
          </form>
          )}

          {config && <p className="mt-10 text-center text-xs text-slate-400">Clinilog v{config.version}</p>}
        </div>
      </section>
    </div>
  );
}

/** First run on a PC: find the school by its code and remember it. */
function SchoolCodeForm({
  disabled,
  onConnected,
  onSuperadmin,
}: {
  disabled: boolean;
  onConnected: (s: SchoolRef) => void;
  onSuperadmin: () => void;
}) {
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const id = useId();

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!code.trim()) {
      setError("Enter your school code.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      onConnected(await api.setSchool(code));
    } catch (err) {
      setError(asCmdError(err).message);
    } finally {
      setBusy(false);
    }
  }

  return (
    <form onSubmit={submit} noValidate className="mt-6 space-y-5">
      <div>
        <label htmlFor={id} className="block text-sm font-medium text-slate-700">
          School code
        </label>
        <input
          id={id}
          value={code}
          onChange={(e) => {
            setCode(e.target.value.toUpperCase());
            setError(null);
          }}
          autoFocus
          autoComplete="off"
          spellCheck={false}
          maxLength={20}
          placeholder="e.g. VNHS"
          aria-invalid={!!error}
          aria-describedby={`${id}-hint`}
          className={`${inputClass(!!error)} font-mono tracking-wider`}
        />
        <p id={`${id}-hint`} className="mt-1.5 text-xs text-slate-500">
          Your Clinilog provider gives each school its own code.
        </p>
      </div>
      {error && (
        <Notice tone="danger" icon={<AlertCircle className="size-4" />}>
          {error}
        </Notice>
      )}
      <button
        type="submit"
        disabled={busy || disabled}
        className="flex w-full items-center justify-center gap-2 rounded-lg bg-teal-700 px-4 py-2.5 text-sm font-semibold text-white hover:bg-teal-800 disabled:cursor-not-allowed disabled:bg-slate-300"
      >
        {busy && <Loader2 className="size-4 animate-spin" aria-hidden />}
        {busy ? "Finding your school…" : "Continue"}
      </button>
      <p className="text-center text-xs text-slate-500">
        Clinilog superadmin?{" "}
        <button type="button" onClick={onSuperadmin} className="font-semibold text-teal-700 hover:underline">
          Log in here
        </button>
      </p>
    </form>
  );
}

function ConnectionStatus({ conn, onRetry }: { conn: Conn; onRetry: () => void }) {
  const [slow, setSlow] = useState(false);
  const checking = conn.state === "checking";
  useEffect(() => {
    if (!checking) return setSlow(false);
    const id = window.setTimeout(() => setSlow(true), SHOW_CHECKING_AFTER_MS);
    return () => window.clearTimeout(id);
  }, [checking]);

  if (conn.state === "online") return null;
  if (conn.state === "checking") {
    if (!slow && conn.attempt === 1) return null;
    return (
      <Notice tone="info" icon={<Loader2 className="size-4 animate-spin" />}>
        Connecting to server…
        {conn.attempt > 1 && <span className="block text-xs opacity-80">The server may take up to a minute to wake up.</span>}
      </Notice>
    );
  }
  return (
    <Notice tone="warn" icon={<WifiOff className="size-4" />}>
      <span className="block">{conn.message}</span>
      <span className="mt-1 block text-xs">
        You can still log in with your school account if this PC has been online before. Changes are saved on this PC and upload
        when the internet is back.
      </span>
      <button
        type="button"
        onClick={onRetry}
        className="mt-2 inline-flex items-center gap-1.5 text-sm font-semibold underline-offset-2 hover:underline"
      >
        <RefreshCw className="size-3.5" aria-hidden /> Try again
      </button>
    </Notice>
  );
}

const NOTICE_TONES = {
  info: "bg-sky-50 text-sky-900 ring-sky-200",
  warn: "bg-amber-50 text-amber-900 ring-amber-200",
  danger: "bg-red-50 text-red-900 ring-red-200",
};

function Notice({
  tone,
  icon,
  children,
}: {
  tone: keyof typeof NOTICE_TONES;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div role={tone === "danger" ? "alert" : "status"} className={`mt-5 flex gap-2 rounded-lg p-3 text-sm ring-1 ${NOTICE_TONES[tone]}`}>
      <span className="mt-0.5 shrink-0" aria-hidden>
        {icon}
      </span>
      <div>{children}</div>
    </div>
  );
}

function FieldError({ id, children }: { id: string; children: React.ReactNode }) {
  return (
    <p id={id} className="mt-1.5 text-xs font-medium text-red-700">
      {children}
    </p>
  );
}

function inputClass(invalid: boolean) {
  return `mt-1 block w-full rounded-lg border bg-white px-3 py-2 text-sm shadow-sm outline-none focus:ring-2 ${
    invalid ? "border-red-400 focus:ring-red-200" : "border-slate-300 focus:border-teal-600 focus:ring-teal-100"
  }`;
}

function formatCountdown(secs: number) {
  const m = Math.floor(secs / 60);
  const s = secs % 60;
  return m > 0 ? `${m}:${String(s).padStart(2, "0")}` : `${s} seconds`;
}
