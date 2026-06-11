import {
  Alert,
  Button,
  Input,
  Modal,
  Popconfirm,
  Space,
  Table,
  Tag,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Eye, RefreshCw, RotateCcw } from "lucide-react";
import { useState } from "react";

import {
  listOutboxEvents,
  retryOutboxEvent,
  type OutboxEventSummary
} from "../../api/admin";
import { SimplePager } from "../../components/SimplePager";
import { useAuthStore } from "../../stores/authStore";
import { dateTime, shortId } from "../../utils/format";
import { tMessage, tOutboxEventType, tStatus } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

const pageSize = 20;

export function OutboxEventsPage() {
  const [status, setStatus] = useState("");
  const [eventType, setEventType] = useState("");
  const [page, setPage] = useState(1);
  const [selected, setSelected] = useState<OutboxEventSummary | null>(null);
  const queryClient = useQueryClient();
  const permissions = useAuthStore((state) => state.permissions);
  const canRetry = hasPermission(permissions, "security:retry_event");

  const query = useQuery({
    queryKey: ["admin", "outbox-events", status, eventType, page],
    queryFn: () =>
      listOutboxEvents({
        status,
        event_type: eventType,
        page,
        page_size: pageSize
      })
  });
  const retryMutation = useMutation({
    mutationFn: retryOutboxEvent,
    onSuccess: () => {
      message.success(tMessage("outbox_event_retry_scheduled"));
      queryClient.invalidateQueries({ queryKey: ["admin", "outbox-events"] });
    }
  });

  const columns: ColumnsType<OutboxEventSummary> = [
    {
      title: "任务",
      dataIndex: "event_type",
      key: "event_type",
      render: (value: string, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text>{tOutboxEventType(value)}</Typography.Text>
          <Typography.Text type="secondary">{shortId(record.id)}</Typography.Text>
        </Space>
      )
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 120,
      render: (value: string) => <StatusTag status={value} />
    },
    {
      title: "租户",
      dataIndex: "tenant_id",
      key: "tenant_id",
      width: 130,
      render: (value?: string | null) => shortId(value)
    },
    {
      title: "尝试次数",
      dataIndex: "attempts",
      key: "attempts",
      width: 100
    },
    {
      title: "下次运行",
      dataIndex: "next_run_at",
      key: "next_run_at",
      width: 180,
      render: (value: string) => dateTime(value)
    },
    {
      title: "创建时间",
      dataIndex: "created_at",
      key: "created_at",
      width: 180,
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 130,
      render: (_, record) => (
        <Space size={6}>
          <Button size="small" icon={<Eye size={14} />} onClick={() => setSelected(record)} />
          <Popconfirm
            title="重试任务"
            okText="重试"
            cancelText="取消"
            disabled={!canRetry || record.status !== "failed"}
            onConfirm={() => retryMutation.mutate(record.id)}
          >
            <Button
              size="small"
              icon={<RotateCcw size={14} />}
              disabled={!canRetry || record.status !== "failed"}
              loading={retryMutation.isPending}
            />
          </Popconfirm>
        </Space>
      )
    }
  ];

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>任务队列</Typography.Title>
          <Typography.Text type="secondary">后台任务、执行状态和重试记录</Typography.Text>
        </div>
        <Space className="page-heading-actions">
          <Input.Search
            allowClear
            placeholder="状态"
            onSearch={(value) => {
              setPage(1);
              setStatus(value);
            }}
            className="table-filter"
          />
          <Input.Search
            allowClear
            placeholder="任务类型编码"
            onSearch={(value) => {
              setPage(1);
              setEventType(value);
            }}
            className="table-search"
          />
          <Button icon={<RefreshCw size={16} />} onClick={() => query.refetch()} />
        </Space>
      </div>

      {query.error ? (
        <Alert type="error" message={tMessage("outbox_events_load_failed")} />
      ) : null}
      {retryMutation.error ? (
        <Alert type="error" message={tMessage("outbox_event_retry_failed")} />
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
        title="任务详情"
        open={Boolean(selected)}
        onCancel={() => setSelected(null)}
        onOk={() => setSelected(null)}
        width={880}
      >
        {selected ? <OutboxEventDetail event={selected} /> : null}
      </Modal>
    </section>
  );
}

function StatusTag({ status }: { status: string }) {
  const color =
    status === "pending"
      ? "blue"
      : status === "processed"
        ? "green"
        : status === "failed"
          ? "red"
          : "default";

  return <Tag color={color}>{tStatus(status)}</Tag>;
}

function OutboxEventDetail({ event }: { event: OutboxEventSummary }) {
  return (
    <Space direction="vertical" size={12} className="audit-detail">
      <Space wrap>
        <Typography.Text strong>
          {tOutboxEventType(event.event_type, { includeCode: true })}
        </Typography.Text>
        <StatusTag status={event.status} />
        <Typography.Text type="secondary">{shortId(event.id)}</Typography.Text>
      </Space>
      <Space direction="vertical" size={2}>
        <Typography.Text type="secondary">调度</Typography.Text>
        <Typography.Text>
          下次运行：{dateTime(event.next_run_at)} · 尝试次数：{event.attempts}
        </Typography.Text>
      </Space>
      <Space direction="vertical" size={2}>
        <Typography.Text type="secondary">处理时间</Typography.Text>
        <Typography.Text>{event.processed_at ? dateTime(event.processed_at) : "-"}</Typography.Text>
      </Space>
      {event.last_error ? (
        <Alert type="error" message={event.last_error} />
      ) : null}
      <Space direction="vertical" size={4} className="audit-json-block">
        <Typography.Text type="secondary">载荷</Typography.Text>
        <pre className="json-view">
          {JSON.stringify(event.payload, null, 2)}
        </pre>
      </Space>
    </Space>
  );
}
