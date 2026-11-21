import {
  Activity,
  BarChart3,
  CreditCard,
  LayoutDashboard,
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
  adminOnly: boolean;
}

// Order matches the spec: staff see the first five, admins see all nine.
export const NAV_ITEMS: NavItem[] = [
  { path: "/", label: "Dashboard", icon: LayoutDashboard, adminOnly: false },
  { path: "/students", label: "Students", icon: Users, adminOnly: false },
  { path: "/visits", label: "Clinic Visits", icon: Stethoscope, adminOnly: false },
  { path: "/search", label: "Search", icon: Search, adminOnly: false },
  { path: "/reports", label: "Reports", icon: BarChart3, adminOnly: false },
  { path: "/users", label: "User Accounts", icon: UserCog, adminOnly: true },
  { path: "/subscription", label: "Subscription", icon: CreditCard, adminOnly: true },
  { path: "/activity", label: "Activity Log", icon: Activity, adminOnly: true },
  { path: "/settings", label: "Settings", icon: Settings, adminOnly: true },
];

export function navFor(role: Role): NavItem[] {
  return NAV_ITEMS.filter((item) => role === "admin" || !item.adminOnly);
}
