import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { api, asCmdError, setUnauthorizedHandler, type Features, type PlanState, type SessionInfo } from "./api";

const PLAN_REFRESH_MS = 30 * 60 * 1000;

type Status = "loading" | "signed_out" | "signed_in";

interface SessionContextValue {
  status: Status;
  session: SessionInfo | null;
  /** Null until fetched, or when it couldn't be fetched and nothing is cached. */
  plan: PlanState | null;
  planError: string | null;
  /** Why the last session ended on its own (e.g. password changed), shown on the login screen. */
  endedReason: string | null;
  signIn: (username: string, password: string) => Promise<void>;
  signOut: () => Promise<void>;
  refreshPlan: () => Promise<void>;
}

const SessionContext = createContext<SessionContextValue | null>(null);

export function SessionProvider({ children }: { children: ReactNode }) {
  const [status, setStatus] = useState<Status>("loading");
  const [session, setSession] = useState<SessionInfo | null>(null);
  const [plan, setPlan] = useState<PlanState | null>(null);
  const [planError, setPlanError] = useState<string | null>(null);
  const [endedReason, setEndedReason] = useState<string | null>(null);

  const reset = useCallback((reason: string | null = null) => {
    setSession(null);
    setPlan(null);
    setPlanError(null);
    // Several requests can fail at once; keep the first (most specific) reason.
    setEndedReason((prev) => (reason && prev ? prev : reason));
    setStatus("signed_out");
  }, []);

  const refreshPlan = useCallback(async () => {
    try {
      setPlan(await api.fetchPlan());
      setPlanError(null);
    } catch (e) {
      const err = asCmdError(e);
      if (err.kind === "unauthorized") reset(err.message);
      else setPlanError(err.message);
    }
  }, [reset]);

  // Any API call that finds the session expired sends the user back to login.
  useEffect(() => {
    setUnauthorizedHandler(reset);
    return () => setUnauthorizedHandler(null);
  }, [reset]);

  // Restore a live session (e.g. after a webview reload), then check the plan.
  useEffect(() => {
    api
      .currentSession()
      .then((s) => {
        if (!s) return reset(null);
        setSession(s);
        setStatus("signed_in");
        void refreshPlan();
      })
      .catch(() => reset(null));
  }, [reset, refreshPlan]);

  useEffect(() => {
    if (status !== "signed_in") return;
    const id = window.setInterval(() => void refreshPlan(), PLAN_REFRESH_MS);
    return () => window.clearInterval(id);
  }, [status, refreshPlan]);

  const signIn = useCallback(
    async (username: string, password: string) => {
      const s = await api.login(username, password); // throws CmdError
      setEndedReason(null);
      setSession(s);
      setStatus("signed_in");
      await refreshPlan();
    },
    [refreshPlan],
  );

  const signOut = useCallback(async () => {
    await api.logout().catch(() => {});
    reset(null);
  }, [reset]);

  const value = useMemo(
    () => ({ status, session, plan, planError, endedReason, signIn, signOut, refreshPlan }),
    [status, session, plan, planError, endedReason, signIn, signOut, refreshPlan],
  );
  return <SessionContext.Provider value={value}>{children}</SessionContext.Provider>;
}

export function useSession(): SessionContextValue {
  const ctx = useContext(SessionContext);
  if (!ctx) throw new Error("useSession must be used inside <SessionProvider>");
  return ctx;
}

const NO_FEATURES: Features = { advanced_search: false, detailed_reports: false, daily_backups: false };

/**
 * Plan flags for the UI. Display only: the server enforces every limit.
 * If the plan is unknown (offline with no cache), fall back to Basic + read-only.
 */
export function usePlan() {
  const { plan, session } = useSession();
  const info = plan?.info ?? null;
  return {
    info,
    isPremium: info?.plan === "premium",
    readOnly: info ? info.read_only : true,
    features: info?.features ?? NO_FEATURES,
    isAdmin: session?.user.role === "admin",
  };
}
