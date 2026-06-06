import { Tag } from "antd";

const colors: Record<string, string> = {
  active: "green",
  disabled: "default",
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
  enterprise: "cyan"
};

export function StatusTag({ value }: { value?: string | null }) {
  if (!value) {
    return <Tag>-</Tag>;
  }

  return <Tag color={colors[value] ?? "blue"}>{value}</Tag>;
}
