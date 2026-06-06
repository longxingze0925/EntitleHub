import { Col, Row, Table, Tag, Typography } from "antd";
import { Activity, FileClock, ShieldCheck, Users } from "lucide-react";

import { useAuthStore } from "../../stores/authStore";

const metrics = [
  {
    label: "当前租户",
    value: "1",
    icon: <ShieldCheck size={20} />
  },
  {
    label: "可用权限",
    value: "permissions",
    icon: <Activity size={20} />
  },
  {
    label: "角色",
    value: "roles",
    icon: <Users size={20} />
  },
  {
    label: "审计",
    value: "enabled",
    icon: <FileClock size={20} />
  }
];

export function DashboardPage() {
  const { user, tenant, roles, permissions } = useAuthStore();
  const rows = [
    {
      key: "user",
      item: "管理员",
      value: user ? `${user.name} / ${user.email}` : "-"
    },
    {
      key: "tenant",
      item: "租户",
      value: tenant?.name ?? "-"
    },
    {
      key: "roles",
      item: "角色",
      value: roles.length > 0 ? roles.join(", ") : "-"
    }
  ];

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>仪表盘</Typography.Title>
          <Typography.Text type="secondary">
            {tenant?.name ?? "当前租户"}
          </Typography.Text>
        </div>
      </div>

      <Row gutter={[12, 12]} className="metric-grid">
        {metrics.map((metric) => {
          const value =
            metric.value === "permissions"
              ? permissions.length
              : metric.value === "roles"
                ? roles.length
                : metric.value === "enabled"
                  ? "已启用"
                  : metric.value;

          return (
            <Col xs={24} sm={12} lg={6} key={metric.label}>
              <div className="metric-tile">
                <span className="metric-icon">{metric.icon}</span>
                <span className="metric-label">{metric.label}</span>
                <strong>{value}</strong>
              </div>
            </Col>
          );
        })}
      </Row>

      <Table
        rowKey="key"
        columns={[
          { title: "项目", dataIndex: "item", key: "item", width: 180 },
          { title: "值", dataIndex: "value", key: "value" }
        ]}
        dataSource={rows}
        pagination={false}
      />

      <div className="permission-strip">
        {permissions.slice(0, 18).map((permission) => (
          <Tag key={permission}>{permission}</Tag>
        ))}
      </div>
    </section>
  );
}
