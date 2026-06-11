import { Alert, Button, DatePicker, Input, Modal, Space, Table, Typography } from "antd";
import type { ColumnsType } from "antd/es/table";
import type { Dayjs } from "dayjs";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Download, Eye, RefreshCw, Search, X } from "lucide-react";
import { useState } from "react";

import {
  exportAuditLogs,
  getAuditLog,
  listAuditLogs,
  type AuditLogDetail,
  type AuditLogQueryParams,
  type AuditLogSummary
} from "../../api/admin";
import { SimplePager } from "../../components/SimplePager";
import { useAuthStore } from "../../stores/authStore";
import { dateTime, shortId } from "../../utils/format";
import { tMessage } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

const pageSize = 20;
const { RangePicker } = DatePicker;

type AuditDateRange = [Dayjs | null, Dayjs | null] | null;

const emptyAuditFilters: Omit<AuditLogQueryParams, "page" | "page_size"> = {
  actor_id: "",
  action: "",
  resource_type: "",
  resource_id: "",
  start_at: undefined,
  end_at: undefined
};

export function AuditLogsPage() {
  const [draftFilters, setDraftFilters] = useState(emptyAuditFilters);
  const [filters, setFilters] = useState(emptyAuditFilters);
  const [dateRange, setDateRange] = useState<AuditDateRange>(null);
  const [page, setPage] = useState(1);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const permissions = useAuthStore((state) => state.permissions);
  const canExport = hasPermission(permissions, "audit:export");
  const query = useQuery({
    queryKey: [
      "admin",
      "audit-logs",
      filters.actor_id,
      filters.action,
      filters.resource_type,
      filters.resource_id,
      filters.start_at,
      filters.end_at,
      page
    ],
    queryFn: () =>
      listAuditLogs({
        ...filters,
        page,
        page_size: pageSize
      })
  });
  const detailQuery = useQuery({
    queryKey: ["admin", "audit-log", selectedId],
    queryFn: () => getAuditLog(selectedId!),
    enabled: Boolean(selectedId)
  });
  const exportMutation = useMutation({
    mutationFn: () => exportAuditLogs(filters),
    onSuccess: (data) => {
      downloadJson(data, `audit-logs-${Date.now()}.json`);
    }
  });

  const applyFilters = () => {
    setPage(1);
    setFilters(draftFilters);
  };

  const resetFilters = () => {
    setPage(1);
    setDraftFilters(emptyAuditFilters);
    setFilters(emptyAuditFilters);
    setDateRange(null);
  };

  const columns: ColumnsType<AuditLogSummary> = [
    {
      title: "动作",
      dataIndex: "action",
      key: "action"
    },
    {
      title: "资源",
      dataIndex: "resource_type",
      key: "resource_type",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text>{record.resource_type}</Typography.Text>
          <Typography.Text type="secondary">{shortId(record.resource_id)}</Typography.Text>
        </Space>
      )
    },
    {
      title: "操作者",
      dataIndex: "actor_type",
      key: "actor_type",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text>{record.actor_type}</Typography.Text>
          <Typography.Text type="secondary">{shortId(record.actor_id)}</Typography.Text>
        </Space>
      )
    },
    {
      title: "IP",
      dataIndex: "ip",
      key: "ip",
      render: (value?: string | null) => value ?? "-"
    },
    {
      title: "请求",
      dataIndex: "request_id",
      key: "request_id",
      render: (value?: string | null) => shortId(value)
    },
    {
      title: "时间",
      dataIndex: "created_at",
      key: "created_at",
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 90,
      render: (_, record) => (
        <Button size="small" icon={<Eye size={14} />} onClick={() => setSelectedId(record.id)}>
          详情
        </Button>
      )
    }
  ];

  const detail = detailQuery.data?.audit_log;

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>审计日志</Typography.Title>
          <Typography.Text type="secondary">关键后台操作、资源变更和追踪信息</Typography.Text>
        </div>
        <Space className="page-heading-actions">
          <Input
            allowClear
            placeholder="操作者 ID"
            value={draftFilters.actor_id}
            onChange={(event) =>
              setDraftFilters((current) => ({ ...current, actor_id: event.target.value }))
            }
            onPressEnter={applyFilters}
            className="audit-filter-input"
          />
          <Input
            allowClear
            placeholder="动作"
            value={draftFilters.action}
            onChange={(event) =>
              setDraftFilters((current) => ({ ...current, action: event.target.value }))
            }
            onPressEnter={applyFilters}
            className="audit-filter-input"
          />
          <Input
            allowClear
            placeholder="资源类型"
            value={draftFilters.resource_type}
            onChange={(event) =>
              setDraftFilters((current) => ({
                ...current,
                resource_type: event.target.value
              }))
            }
            onPressEnter={applyFilters}
            className="audit-filter-input"
          />
          <Input
            allowClear
            placeholder="资源 ID"
            value={draftFilters.resource_id}
            onChange={(event) =>
              setDraftFilters((current) => ({ ...current, resource_id: event.target.value }))
            }
            onPressEnter={applyFilters}
            className="audit-filter-input"
          />
          <RangePicker
            showTime
            value={dateRange}
            onChange={(values) => {
              const nextRange: AuditDateRange = values ? [values[0], values[1]] : null;
              setDateRange(nextRange);
              setDraftFilters((current) => ({
                ...current,
                start_at: nextRange?.[0]?.toISOString(),
                end_at: nextRange?.[1]?.toISOString()
              }));
            }}
            className="audit-range-filter"
          />
          <Button type="primary" icon={<Search size={16} />} onClick={applyFilters}>
            查询
          </Button>
          <Button icon={<X size={16} />} onClick={resetFilters}>
            清空
          </Button>
          <Button icon={<RefreshCw size={16} />} onClick={() => query.refetch()} />
          <Button
            icon={<Download size={16} />}
            disabled={!canExport}
            loading={exportMutation.isPending}
            onClick={() => exportMutation.mutate()}
          >
            导出
          </Button>
        </Space>
      </div>
      {query.error ? (
        <Alert type="error" message={tMessage("audit_logs_load_failed")} />
      ) : null}
      {detailQuery.error ? (
        <Alert type="error" message={tMessage("audit_log_detail_failed")} />
      ) : null}
      {exportMutation.error ? (
        <Alert type="error" message={tMessage("audit_logs_export_failed")} />
      ) : null}
      <Table
        rowKey="id"
        loading={query.isLoading}
        columns={columns}
        dataSource={query.data?.items ?? []}
        pagination={false}
        locale={{ emptyText: "暂无数据" }}
      />
      <SimplePager
        page={page}
        pageSize={pageSize}
        itemCount={query.data?.items.length ?? 0}
        loading={query.isFetching}
        onChange={setPage}
      />

      <Modal
        title="审计详情"
        open={Boolean(selectedId)}
        onCancel={() => setSelectedId(null)}
        onOk={() => setSelectedId(null)}
        width={880}
      >
        {detailQuery.isLoading ? (
          <Typography.Text type="secondary">加载中</Typography.Text>
        ) : detail ? (
          <AuditDetailView detail={detail} />
        ) : null}
      </Modal>
    </section>
  );
}

function downloadJson(value: unknown, fileName: string) {
  const blob = new Blob([JSON.stringify(value, null, 2)], {
    type: "application/json"
  });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileName;
  link.click();
  URL.revokeObjectURL(url);
}

function AuditDetailView({ detail }: { detail: AuditLogDetail }) {
  return (
    <Space direction="vertical" size={12} className="audit-detail">
      <Space wrap>
        <Typography.Text strong>{detail.action}</Typography.Text>
        <Typography.Text type="secondary">{detail.resource_type}</Typography.Text>
        <Typography.Text type="secondary">{shortId(detail.resource_id)}</Typography.Text>
        <Typography.Text type="secondary">{dateTime(detail.created_at)}</Typography.Text>
      </Space>
      <Space direction="vertical" size={2}>
        <Typography.Text type="secondary">操作者</Typography.Text>
        <Typography.Text>
          {detail.actor_type} · {shortId(detail.actor_id)}
        </Typography.Text>
      </Space>
      <Space direction="vertical" size={2}>
        <Typography.Text type="secondary">请求</Typography.Text>
        <Typography.Text copyable={Boolean(detail.request_id)}>
          {detail.request_id ?? "-"}
        </Typography.Text>
      </Space>
      <JsonBlock title="变更前" value={detail.before_json} />
      <JsonBlock title="变更后" value={detail.after_json} />
      <JsonBlock title="元数据" value={detail.metadata_json} />
    </Space>
  );
}

function JsonBlock({ title, value }: { title: string; value: unknown }) {
  return (
    <Space direction="vertical" size={4} className="audit-json-block">
      <Typography.Text type="secondary">{title}</Typography.Text>
      <pre className="json-view">
        {value === null || value === undefined ? "-" : JSON.stringify(value, null, 2)}
      </pre>
    </Space>
  );
}
