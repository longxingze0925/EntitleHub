import {
  Activity,
  AppWindow,
  BellRing,
  Boxes,
  ClipboardList,
  CreditCard,
  FileClock,
  Gauge,
  Inbox,
  KeyRound,
  ScrollText,
  Settings,
  Shield,
  ShieldCheck,
  Users
} from "lucide-react";
import type { ReactNode } from "react";

export interface MenuRoute {
  key: string;
  path: string;
  label: string;
  permission?: string;
  icon: ReactNode;
}

export const menuRoutes: MenuRoute[] = [
  {
    key: "dashboard",
    path: "/",
    label: "仪表盘",
    icon: <Gauge size={18} />
  },
  {
    key: "team",
    path: "/team",
    label: "团队成员",
    permission: "member:read",
    icon: <Users size={18} />
  },
  {
    key: "roles",
    path: "/roles",
    label: "角色权限",
    permission: "role:read",
    icon: <Shield size={18} />
  },
  {
    key: "customers",
    path: "/customers",
    label: "客户管理",
    permission: "customer:read",
    icon: <Boxes size={18} />
  },
  {
    key: "apps",
    path: "/apps",
    label: "应用管理",
    permission: "app:read",
    icon: <AppWindow size={18} />
  },
  {
    key: "licenses",
    path: "/licenses",
    label: "授权管理",
    permission: "license:read",
    icon: <KeyRound size={18} />
  },
  {
    key: "subscriptions",
    path: "/subscriptions",
    label: "订阅管理",
    permission: "subscription:read",
    icon: <CreditCard size={18} />
  },
  {
    key: "devices",
    path: "/devices",
    label: "设备管理",
    permission: "device:read",
    icon: <Activity size={18} />
  },
  {
    key: "releases",
    path: "/releases",
    label: "版本管理",
    permission: "release:read",
    icon: <ClipboardList size={18} />
  },
  {
    key: "scripts",
    path: "/scripts",
    label: "脚本管理",
    permission: "script:read",
    icon: <ScrollText size={18} />
  },
  {
    key: "audit",
    path: "/audit",
    label: "审计日志",
    permission: "audit:read",
    icon: <FileClock size={18} />
  },
  {
    key: "system-settings",
    path: "/system-settings",
    label: "系统配置",
    permission: "system:read",
    icon: <Settings size={18} />
  },
  {
    key: "notification-channels",
    path: "/notification-channels",
    label: "通知渠道",
    permission: "notification:read",
    icon: <BellRing size={18} />
  },
  {
    key: "outbox",
    path: "/outbox",
    label: "任务队列",
    permission: "security:view_events",
    icon: <Inbox size={18} />
  },
  {
    key: "security",
    path: "/security",
    label: "安全状态",
    icon: <ShieldCheck size={18} />
  }
];
