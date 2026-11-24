import { Loader2 } from "lucide-react";
import type { ReactNode } from "react";
import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { AppLayout } from "./components/AppLayout";
import { SessionProvider, useSession } from "./lib/session";
import { SyncProvider } from "./lib/sync";
import { DashboardPage } from "./pages/Dashboard";
import { LoginPage } from "./pages/Login";
import { ReportsPage } from "./pages/Reports";
import { ActivityLogPage } from "./pages/ActivityLog";
import { SearchPage } from "./pages/Search";
import { UserAccountsPage } from "./pages/UserAccounts";
import { SettingsPage } from "./pages/Settings";
import { StudentDetailPage } from "./pages/students/StudentDetail";
import { StudentFormPage } from "./pages/students/StudentForm";
import { StudentsListPage } from "./pages/students/StudentsList";
import { VisitDetailPage } from "./pages/visits/VisitDetail";
import { VisitFormPage } from "./pages/visits/VisitForm";
import { VisitsListPage } from "./pages/visits/VisitsList";
import { SubscriptionPage } from "./pages/Subscription";
import { PaymentRequestsPage } from "./pages/superadmin/PaymentRequests";
import { ReviewPaymentPage } from "./pages/superadmin/ReviewPayment";
import { SchoolsPage } from "./pages/superadmin/Schools";

export default function App() {
  return (
    <SessionProvider>
      <SyncProvider>
        <HashRouter>
          <Root />
        </HashRouter>
      </SyncProvider>
    </SessionProvider>
  );
}

function Root() {
  const { status, session } = useSession();

  if (status === "loading") {
    return (
      <div className="flex h-full items-center justify-center text-slate-500">
        <Loader2 className="size-6 animate-spin" aria-label="Loading" />
      </div>
    );
  }
  if (status === "signed_out") return <LoginPage />;

  if (session?.user.role === "superadmin") {
    return (
      <Routes>
        <Route element={<AppLayout />}>
          <Route index element={<PaymentRequestsPage />} />
          <Route path="requests/:id" element={<ReviewPaymentPage />} />
          <Route path="schools" element={<SchoolsPage />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Route>
      </Routes>
    );
  }

  return (
    <Routes>
      <Route element={<AppLayout />}>
        <Route index element={<DashboardPage />} />
        <Route path="students" element={<StudentsListPage />} />
        <Route path="students/new" element={<StudentFormPage key="new" />} />
        <Route path="students/:id" element={<StudentDetailPage />} />
        <Route path="students/:id/edit" element={<StudentFormPage key="edit" />} />
        <Route path="visits" element={<VisitsListPage />} />
        <Route path="visits/new" element={<VisitFormPage key="new" />} />
        <Route path="visits/:id" element={<VisitDetailPage />} />
        <Route path="visits/:id/edit" element={<VisitFormPage key="edit" />} />
        <Route path="search" element={<SearchPage />} />
        <Route path="reports" element={<ReportsPage />} />
        <Route path="users" element={<AdminOnly><UserAccountsPage /></AdminOnly>} />
        <Route path="subscription" element={<AdminOnly><SubscriptionPage /></AdminOnly>} />
        <Route path="activity" element={<AdminOnly><ActivityLogPage /></AdminOnly>} />
        <Route path="settings" element={<AdminOnly><SettingsPage /></AdminOnly>} />
        <Route path="*" element={<Navigate to="/" replace />} />
      </Route>
    </Routes>
  );
}

/** UI guard only; the server also rejects admin endpoints for staff. */
function AdminOnly({ children }: { children: ReactNode }) {
  const { session } = useSession();
  return session?.user.role === "admin" ? <>{children}</> : <Navigate to="/" replace />;
}
