import { Button, Space, Table, Tag, Typography } from "antd";
import { Plus, RefreshCw } from "lucide-react";

import { tStatus } from "../../utils/i18n";

export interface WorkspacePageProps {
  title: string;
  subtitle: string;
  primaryAction?: string;
  permissions?: string[];
}

const columns = [
  {
    title: "名称",
    dataIndex: "name",
    key: "name"
  },
  {
    title: "状态",
    dataIndex: "status",
    key: "status",
    render: (status: string) => <Tag color="blue">{tStatus(status)}</Tag>
  },
  {
    title: "更新时间",
    dataIndex: "updatedAt",
    key: "updatedAt"
  }
];

export function WorkspacePage({
  title,
  subtitle,
  primaryAction,
  permissions = []
}: WorkspacePageProps) {
  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>{title}</Typography.Title>
          <Typography.Text type="secondary">{subtitle}</Typography.Text>
        </div>
        <Space>
          <Button icon={<RefreshCw size={16} />} />
          {primaryAction ? (
            <Button type="primary" icon={<Plus size={16} />}>
              {primaryAction}
            </Button>
          ) : null}
        </Space>
      </div>

      <div className="permission-strip">
        {permissions.map((permission) => (
          <Tag key={permission}>{permission}</Tag>
        ))}
      </div>

      <Table
        rowKey="name"
        columns={columns}
        dataSource={[]}
        pagination={false}
        locale={{ emptyText: "暂无数据" }}
      />
    </section>
  );
}
