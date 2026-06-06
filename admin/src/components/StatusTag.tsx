import { Tag } from "antd";

import { tStatus } from "../utils/i18n";

const colors: Record<string, string> = {
  active: "green",
  archived: "default",
  disabled: "default",
  not_started: "default",
  trialing: "cyan",
  past_due: "orange",
  cancelled: "red",
  expired: "default",
  suspended: "orange",
  revoked: "red",
  blacklisted: "red",
  draft: "default",
  published: "green",
  deprecated: "orange",
  standard: "blue",
  trial: "purple",
  enterprise: "cyan",
  pending: "blue",
  processed: "green",
  failed: "red",
  unbound: "default"
};

export function StatusTag({ value }: { value?: string | null }) {
  if (!value) {
    return <Tag>-</Tag>;
  }

  return <Tag color={colors[value] ?? "blue"}>{tStatus(value)}</Tag>;
}
