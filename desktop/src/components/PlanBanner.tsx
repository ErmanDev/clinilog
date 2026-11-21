import { AlertTriangle, CloudOff, Info } from "lucide-react";
import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { formatDate } from "../lib/format";
import { useSession } from "../lib/session";

const EXPIRY_WARNING_DAYS = 14;

/** Shows the most important plan notice, if any, above the page content. */
export function PlanBanner() {
  const { plan, planError, session } = useSession();
  const isAdmin = session?.user.role === "admin";

  if (!plan) {
    if (!planError) return null;
    return (
      <Banner tone="warn" icon={<CloudOff className="size-4" />}>
        Couldn't check your school's plan: {planError} You can view records, but changes are paused until the app
        reaches the server.
      </Banner>
    );
  }

  const { info } = plan;

  if (plan.source === "cache" && plan.grace_expired) {
    return (
      <Banner tone="danger" icon={<CloudOff className="size-4" />}>
        The app has been offline for more than 7 days. Records are view-only until it reconnects to the server.
      </Banner>
    );
  }

  if (info.read_only) {
    const renew = isAdmin ? (
      <Link to="/subscription" className="font-semibold underline">
        Renew now
      </Link>
    ) : (
      "Ask your clinic admin to renew."
    );
    const what = info.status === "none" ? "Your school has no active subscription." : "Your subscription has expired.";
    return (
      <Banner tone="danger" icon={<AlertTriangle className="size-4" />}>
        {what} You can still view records, but can't add or change them. {renew}
      </Banner>
    );
  }

  if (plan.source === "cache" && plan.grace_ends_at) {
    return (
      <Banner tone="warn" icon={<CloudOff className="size-4" />}>
        Offline — using your school's last known plan. Changes stay allowed until {formatDate(plan.grace_ends_at)}.
      </Banner>
    );
  }

  if (info.days_left !== null && info.days_left <= EXPIRY_WARNING_DAYS && info.end_date) {
    return (
      <Banner tone="info" icon={<Info className="size-4" />}>
        Your subscription ends on {formatDate(info.end_date)} ({info.days_left} day{info.days_left === 1 ? "" : "s"}{" "}
        left).
      </Banner>
    );
  }

  return null;
}

const TONES = {
  info: "bg-sky-50 text-sky-900 ring-sky-200",
  warn: "bg-amber-50 text-amber-900 ring-amber-200",
  danger: "bg-red-50 text-red-900 ring-red-200",
};

function Banner({ tone, icon, children }: { tone: keyof typeof TONES; icon: ReactNode; children: ReactNode }) {
  return (
    <div role="status" className={`mx-8 mt-4 flex items-start gap-2 rounded-lg px-4 py-3 text-sm ring-1 ${TONES[tone]}`}>
      <span className="mt-0.5 shrink-0" aria-hidden>
        {icon}
      </span>
      <p>{children}</p>
    </div>
  );
}
