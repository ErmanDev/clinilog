import {
  Activity,
  BarChart3,
  Building2,
  CreditCard,
  LayoutDashboard,
  ReceiptText,
  Search,
  Settings,
  Stethoscope,
  Users,
  UserCog,
  type LucideIcon,
} from "lucide-react";
import type { Role } from "./api";

export interface NavItem {
  path: string;
  label: string;
  icon: LucideIcon;
  roles: Role[];
}

const ALL: Role[] = ["admin", "staff"];
const ADMIN: Role[] = ["admin"];
const SUPER: Role[] = ["superadmin"];

// Order matches the spec: staff see the first five, admins see all nine.
// The superadmin only reviews payments.
export const NAV_ITEMS: NavItem[] = [
  { path: "/", label: "Dashboard", icon: LayoutDashboard, roles: ALL },
  { path: "/students", label: "Students", icon: Users, roles: ALL },
  { path: "/visits", label: "Clinic Visits", icon: Stethoscope, roles: ALL },
  { path: "/search", label: "Search", icon: Search, roles: ALL },
  { path: "/reports", label: "Reports", icon: BarChart3, roles: ALL },
  { path: "/users", label: "User Accounts", icon: UserCog, roles: ADMIN },
  { path: "/subscription", label: "Subscription", icon: CreditCard, roles: ADMIN },
  { path: "/activity", label: "Activity Log", icon: Activity, roles: ADMIN },
  { path: "/settings", label: "Settings", icon: Settings, roles: ADMIN },
  { path: "/", label: "Payment Requests", icon: ReceiptText, roles: SUPER },
  { path: "/schools", label: "Schools", icon: Building2, roles: SUPER },
];

export function navFor(role: Role): NavItem[] {
  return NAV_ITEMS.filter((item) => item.roles.includes(role));
}
