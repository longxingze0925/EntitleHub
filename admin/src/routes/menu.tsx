import {
  Activity,
  AppWindow,
  BellRing,
  Boxes,
  ClipboardList,
  CreditCard,
  Cpu,
  FileClock,
  Gauge,
  Inbox,
  KeyRound,
  ListTree,
  PackageOpen,
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
  icon?: ReactNode;
  children?: MenuRoute[];
}

export const menuRoutes: MenuRoute[] = [
  {
    key: "dashboard",
    path: "/",
    label: "仪表盘",
    icon: <Gauge size={18} />
  },
  {
    key: "organization-access",
    path: "/team",
    label: "组织权限",
    icon: <Shield size={18} />,
    children: [
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
      }
    ]
  },
  {
    key: "customer-apps",
    path: "/customers",
    label: "客户与应用",
    icon: <Boxes size={18} />,
    children: [
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
      }
    ]
  },
  {
    key: "ai-billing",
    path: "/ai-billing/providers",
    label: "接口计费",
    permission: "ai:read",
    icon: <Cpu size={18} />,
    children: [
      {
        key: "ai-billing-providers",
        path: "/ai-billing/providers",
        label: "渠道管理",
        permission: "ai:read",
        icon: <Cpu size={18} />
      },
      {
        key: "ai-billing-models",
        path: "/ai-billing/models",
        label: "模型价格",
        permission: "ai:read",
        icon: <ClipboardList size={18} />
      },
      {
        key: "ai-billing-wallets",
        path: "/ai-billing/wallets",
        label: "客户余额",
        permission: "ai:read",
        icon: <CreditCard size={18} />
      }
    ]
  },
  {
    key: "publishing",
    path: "/releases",
    label: "发布分发",
    icon: <PackageOpen size={18} />,
    children: [
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
      }
    ]
  },
  {
    key: "tasks-logs",
    path: "/tasks",
    label: "任务与日志",
    icon: <ListTree size={18} />,
    children: [
      {
        key: "tasks-center",
        path: "/tasks",
        label: "任务中心",
        permission: "security:view_events",
        icon: <Inbox size={18} />
      },
      {
        key: "logs-ai-jobs",
        path: "/logs/ai-jobs",
        label: "生成任务",
        permission: "ai:job:read",
        icon: <ClipboardList size={18} />
      },
      {
        key: "logs-ai-usage",
        path: "/logs/ai-usage",
        label: "调用日志",
        permission: "ai:read",
        icon: <FileClock size={18} />
      },
      {
        key: "logs-billing-ledger",
        path: "/logs/billing-ledger",
        label: "计费流水",
        permission: "ai:read",
        icon: <CreditCard size={18} />
      },
      {
        key: "logs-ai-assets",
        path: "/logs/ai-assets",
        label: "缓存素材",
        permission: "ai:read",
        icon: <Boxes size={18} />
      },
      {
        key: "logs-audit",
        path: "/logs/audit",
        label: "审计日志",
        permission: "audit:read",
        icon: <FileClock size={18} />
      }
    ]
  },
  {
    key: "system-admin",
    path: "/system-settings",
    label: "系统设置",
    icon: <Settings size={18} />,
    children: [
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
        key: "security",
        path: "/security",
        label: "安全状态",
        icon: <ShieldCheck size={18} />
      }
    ]
  }
];

export const flatMenuRoutes = flattenMenuRoutes(menuRoutes);

function flattenMenuRoutes(routes: MenuRoute[]): MenuRoute[] {
  return routes.flatMap((route) => [
    route,
    ...(route.children ? flattenMenuRoutes(route.children) : [])
  ]);
}
