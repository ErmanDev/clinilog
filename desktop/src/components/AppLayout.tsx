import { LogOut } from "lucide-react";
import logoMark from "../assets/clinilog-mark.png";
import { useState } from "react";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import { planLabel } from "../lib/format";
import { navFor } from "../lib/nav";
import { useSession } from "../lib/session";
import { useSync } from "../lib/sync";
import { ConfirmDialog } from "./Dialog";
import { OfflineBanner, SyncIndicator } from "./SyncIndicator";
import { WindowControls } from "./WindowControls";
import { ErrorBoundary } from "./ErrorBoundary";
import { PlanBanner } from "./PlanBanner";

export function AppLayout() {
  const { session, plan, signOut } = useSession();
  const location = useLocation();
  const { status } = useSync();
  const [confirmLogout, setConfirmLogout] = useState(false);
  if (!session) return null;
  const waiting = (status?.pending ?? 0) + (status?.failed ?? 0);

  const superadmin = session.user.role === "superadmin";
  const items = navFor(session.user.role);
  // "/students/123/edit" belongs to "Students": pick the longest matching nav path.
  const current = items.filter(
    (i) => location.pathname === i.path || (i.path !== "/" && location.pathname.startsWith(`${i.path}/`)),
  ).sort((a, b) => b.path.length - a.path.length)[0];

  return (
    <div className="flex h-full print:block print:h-auto">
      <aside className="flex w-60 shrink-0 flex-col border-r border-slate-200 bg-white print:hidden">
        <div className="flex items-center gap-2.5 px-5 py-5">
          <img src={logoMark} alt="" className="size-9 shrink-0" draggable={false} />
          <div className="min-w-0">
            <p className="text-base font-bold leading-tight tracking-tight">Clinilog</p>
            <p className="truncate text-xs text-slate-500" title={session.school.name}>
              {superadmin ? "Superadmin" : session.school.name}
            </p>
          </div>
        </div>

        <nav aria-label="Main" className="flex-1 space-y-0.5 overflow-y-auto px-3">
          {items.map(({ path, label, icon: Icon }) => (
            <NavLink
              key={path}
              to={path}
              end={path === "/"}
              className={({ isActive }) =>
                `flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium transition-colors ${
                  isActive ? "bg-teal-50 text-teal-800" : "text-slate-600 hover:bg-slate-100 hover:text-slate-900"
                }`
              }
            >
              <Icon className="size-4.5 shrink-0" aria-hidden />
              {label}
            </NavLink>
          ))}
        </nav>

        {!superadmin && <SyncIndicator />}
        <div className="border-t border-slate-200 p-3">
          <div className="px-2 py-1.5">
            <p className="truncate text-sm font-medium" title={session.user.full_name}>
              {session.user.full_name}
            </p>
            <p className="text-xs capitalize text-slate-500">{session.user.role}</p>
          </div>
          <button
            type="button"
            onClick={() => (waiting > 0 ? setConfirmLogout(true) : void signOut())}
            className="mt-1 flex w-full items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium text-slate-600 hover:bg-slate-100 hover:text-slate-900"
          >
            <LogOut className="size-4.5" aria-hidden />
            Log out
          </button>
        </div>
      </aside>

      <div className="flex min-w-0 flex-1 flex-col print:block">
        <header className="flex h-16 shrink-0 items-center justify-between border-b border-slate-200 bg-white px-8 print:hidden">
          <h1 className="text-lg font-semibold">{superadmin && location.pathname.startsWith("/requests/") ? "Review payment" : current?.label ?? "Clinilog"}</h1>
          <div className="flex items-center gap-3">
          {plan && (
            <span
              className={`rounded-full px-2.5 py-1 text-xs font-semibold ring-1 ${
                plan.info.plan === "premium"
                  ? "bg-amber-50 text-amber-800 ring-amber-300"
                  : "bg-slate-100 text-slate-700 ring-slate-300"
              }`}
            >
              {planLabel(plan.info.plan)} plan{plan.info.is_trial && " · trial"}
            </span>
          )}
          <WindowControls />
          </div>
        </header>
        {!superadmin && (
          <div className="print:hidden">
            <OfflineBanner />
            <PlanBanner />
          </div>
        )}
        <main className="flex-1 overflow-y-auto px-8 py-6 print:overflow-visible print:p-0">
          <ErrorBoundary resetKey={location.pathname}>
            <Outlet />
          </ErrorBoundary>
        </main>
        <ConfirmDialog
          open={confirmLogout}
          title="Log out?"
          confirmLabel="Log out"
          tone="primary"
          onCancel={() => setConfirmLogout(false)}
          onConfirm={() => {
            setConfirmLogout(false);
            void signOut();
          }}
        >
          <p>
            {waiting} change{waiting === 1 ? " hasn't" : "s haven't"} been uploaded yet. {waiting === 1 ? "It stays" : "They stay"} saved
            (encrypted) on this PC and upload the next time you log in here while online.
          </p>
        </ConfirmDialog>
      </div>
    </div>
  );
}
