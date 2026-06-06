import {
  Alert,
  Button,
  Form,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Tooltip,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { BellRing, Pencil, Plus, RefreshCw, Send, ShieldCheck } from "lucide-react";
import { useState } from "react";

import {
  createNotificationChannel,
  listNotificationChannels,
  testNotificationChannel,
  updateNotificationChannel,
  type NotificationChannel,
  type NotificationChannelKind
} from "../../api/admin";
import { useAuthStore } from "../../stores/authStore";
import { dateTime } from "../../utils/format";
import { hasPermission } from "../../utils/permissions";

interface ChannelFormValues {
  name: string;
  kind: NotificationChannelKind;
  enabled: boolean;
  webhook_url?: string;
  smtp_host?: string;
  smtp_port?: number;
  smtp_user?: string;
  smtp_password?: string;
  from?: string;
  to?: string;
  pagerduty_service?: string;
  pagerduty_routing_key?: string;
}

type TestMode = "dry_run" | "delivery";

const channelKindOptions = [
  { label: "Webhook", value: "webhook" },
  { label: "Email", value: "email" },
  { label: "PagerDuty", value: "pagerduty" }
];

export function NotificationChannelsPage() {
  const [form] = Form.useForm<ChannelFormValues>();
  const [editing, setEditing] = useState<NotificationChannel | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const queryClient = useQueryClient();
  const permissions = useAuthStore((state) => state.permissions);
  const canUpdate = hasPermission(permissions, "notification:update");
  const selectedKind = Form.useWatch("kind", form) ?? "webhook";

  const query = useQuery({
    queryKey: ["admin", "notification-channels"],
    queryFn: listNotificationChannels
  });

  const saveMutation = useMutation({
    mutationFn: (values: ChannelFormValues) => {
      const payload = buildPayload(values, Boolean(editing));
      if (editing) {
        return updateNotificationChannel({
          id: editing.id,
          payload
        });
      }

      return createNotificationChannel({
        name: payload.name ?? values.name.trim(),
        kind: values.kind,
        enabled: payload.enabled,
        config: payload.config ?? {},
        secret: payload.secret
      });
    },
    onSuccess: () => {
      message.success("notification_channel_saved");
      setModalOpen(false);
      setEditing(null);
      form.resetFields();
      queryClient.invalidateQueries({
        queryKey: ["admin", "notification-channels"]
      });
    }
  });

  const testMutation = useMutation({
    mutationFn: ({ id, mode }: { id: string; mode: TestMode }) =>
      testNotificationChannel(id, {
        mode,
        confirm_delivery: mode === "delivery"
      }),
    onSuccess: (_, variables) => {
      message.success(
        variables.mode === "delivery"
          ? "notification_channel_test_sent"
          : "notification_channel_test_passed"
      );
      queryClient.invalidateQueries({
        queryKey: ["admin", "notification-channels"]
      });
    },
    onError: () => {
      queryClient.invalidateQueries({
        queryKey: ["admin", "notification-channels"]
      });
    }
  });

  const openCreate = () => {
    setEditing(null);
    form.setFieldsValue({
      name: "",
      kind: "webhook",
      enabled: true,
      smtp_port: 587
    });
    setModalOpen(true);
  };

  const openEdit = (channel: NotificationChannel) => {
    setEditing(channel);
    form.setFieldsValue(toFormValues(channel));
    setModalOpen(true);
  };

  const confirmDeliveryTest = (channel: NotificationChannel) => {
    Modal.confirm({
      title: "发送测试消息",
      content: "会向当前渠道发送一条真实测试消息，外部 Webhook、SMTP 或 PagerDuty 可能收到通知。",
      okText: "发送",
      cancelText: "取消",
      onOk: () => testMutation.mutateAsync({ id: channel.id, mode: "delivery" })
    });
  };

  const columns: ColumnsType<NotificationChannel> = [
    {
      title: "名称",
      dataIndex: "name",
      key: "name",
      render: (value: string, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text>{value}</Typography.Text>
          <Typography.Text type="secondary">{record.id.slice(0, 8)}</Typography.Text>
        </Space>
      )
    },
    {
      title: "类型",
      dataIndex: "kind",
      key: "kind",
      width: 120,
      render: (value: NotificationChannelKind) => <KindTag kind={value} />
    },
    {
      title: "状态",
      dataIndex: "enabled",
      key: "enabled",
      width: 90,
      render: (value: boolean) => (
        <Tag color={value ? "green" : "default"}>{value ? "enabled" : "disabled"}</Tag>
      )
    },
    {
      title: "目标",
      dataIndex: "config",
      key: "target",
      render: (value: Record<string, unknown>) => (
        <Typography.Text>{targetSummary(value)}</Typography.Text>
      )
    },
    {
      title: "密钥",
      dataIndex: "secret_configured",
      key: "secret_configured",
      width: 90,
      render: (value: boolean) => (
        <Tag color={value ? "blue" : "red"}>{value ? "set" : "missing"}</Tag>
      )
    },
    {
      title: "最近测试",
      key: "last_test",
      width: 180,
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <TestStatusTag status={record.last_test_status} />
          <Typography.Text type="secondary">
            {record.last_test_at ? dateTime(record.last_test_at) : "-"}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "更新时间",
      dataIndex: "updated_at",
      key: "updated_at",
      width: 180,
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 150,
      render: (_, record) => (
        <Space size={6}>
          <Tooltip title="编辑">
            <Button
              size="small"
              icon={<Pencil size={14} />}
              disabled={!canUpdate}
              onClick={() => openEdit(record)}
            />
          </Tooltip>
          <Tooltip title="校验配置">
            <Button
              size="small"
              icon={<ShieldCheck size={14} />}
              disabled={!canUpdate || !record.enabled || !record.secret_configured}
              loading={
                testMutation.isPending &&
                testMutation.variables?.id === record.id &&
                testMutation.variables.mode === "dry_run"
              }
              onClick={() => testMutation.mutate({ id: record.id, mode: "dry_run" })}
            />
          </Tooltip>
          <Tooltip title="发送测试消息">
            <Button
              size="small"
              icon={<Send size={14} />}
              disabled={!canUpdate || !record.enabled || !record.secret_configured}
              loading={
                testMutation.isPending &&
                testMutation.variables?.id === record.id &&
                testMutation.variables.mode === "delivery"
              }
              onClick={() => confirmDeliveryTest(record)}
            />
          </Tooltip>
        </Space>
      )
    }
  ];

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>通知渠道</Typography.Title>
          <Typography.Text type="secondary">Webhook / SMTP / PagerDuty</Typography.Text>
        </div>
        <Space>
          <Button icon={<RefreshCw size={16} />} onClick={() => query.refetch()} />
          <Button
            type="primary"
            icon={<Plus size={16} />}
            disabled={!canUpdate}
            onClick={openCreate}
          >
            新增
          </Button>
        </Space>
      </div>

      {query.error ? <Alert type="error" message="notification_channels_load_failed" /> : null}
      {saveMutation.error ? <Alert type="error" message="notification_channel_save_failed" /> : null}
      {testMutation.error ? (
        <Alert type="error" message="notification_channel_test_failed" />
      ) : null}

      <Table
        rowKey="id"
        loading={query.isLoading}
        columns={columns}
        dataSource={query.data?.items ?? []}
        pagination={false}
        locale={{ emptyText: "暂无数据" }}
      />

      <Modal
        title={
          <Space size={8}>
            <BellRing size={18} />
            <span>{editing ? "编辑通知渠道" : "新增通知渠道"}</span>
          </Space>
        }
        open={modalOpen}
        onCancel={() => {
          setModalOpen(false);
          setEditing(null);
        }}
        onOk={() => form.submit()}
        confirmLoading={saveMutation.isPending}
        width={720}
        destroyOnClose
      >
        <Form<ChannelFormValues>
          form={form}
          layout="vertical"
          onFinish={(values) => saveMutation.mutate(values)}
        >
          <div className="settings-grid-inner">
            <Form.Item
              name="name"
              label="名称"
              rules={[{ required: true, message: "请输入名称" }]}
            >
              <Input />
            </Form.Item>
            <Form.Item name="enabled" label="启用" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item
              name="kind"
              label="类型"
              rules={[{ required: true, message: "请选择类型" }]}
            >
              <Select disabled={Boolean(editing)} options={channelKindOptions} />
            </Form.Item>
          </div>

          {selectedKind === "webhook" ? (
            <Form.Item
              name="webhook_url"
              label="Webhook URL"
              rules={[
                {
                  required: !editing,
                  message: "请输入 Webhook URL"
                },
                { type: "url", message: "URL 格式不正确" }
              ]}
            >
              <Input.Password placeholder={editing?.secret_configured ? "已配置" : ""} />
            </Form.Item>
          ) : null}

          {selectedKind === "email" ? (
            <div className="settings-grid-inner">
              <Form.Item
                name="smtp_host"
                label="SMTP Host"
                rules={[{ required: true, message: "请输入 SMTP Host" }]}
              >
                <Input />
              </Form.Item>
              <Form.Item
                name="smtp_port"
                label="SMTP Port"
                rules={[{ required: true, message: "请输入 SMTP Port" }]}
              >
                <InputNumber min={1} max={65535} className="form-number" />
              </Form.Item>
              <Form.Item
                name="smtp_user"
                label="SMTP User"
                rules={[{ required: true, message: "请输入 SMTP User" }]}
              >
                <Input />
              </Form.Item>
              <Form.Item
                name="smtp_password"
                label="SMTP Password"
                rules={[
                  {
                    required: !editing,
                    message: "请输入 SMTP Password"
                  }
                ]}
              >
                <Input.Password placeholder={editing?.secret_configured ? "已配置" : ""} />
              </Form.Item>
              <Form.Item
                name="from"
                label="From"
                rules={[
                  { required: true, message: "请输入 From" },
                  { type: "email", message: "邮箱格式不正确" }
                ]}
              >
                <Input />
              </Form.Item>
              <Form.Item
                name="to"
                label="To"
                rules={[{ required: true, message: "请输入 To" }]}
              >
                <Input />
              </Form.Item>
            </div>
          ) : null}

          {selectedKind === "pagerduty" ? (
            <div className="settings-grid-inner">
              <Form.Item name="pagerduty_service" label="Service">
                <Input />
              </Form.Item>
              <Form.Item
                name="pagerduty_routing_key"
                label="Routing Key"
                rules={[
                  {
                    required: !editing,
                    message: "请输入 Routing Key"
                  }
                ]}
              >
                <Input.Password placeholder={editing?.secret_configured ? "已配置" : ""} />
              </Form.Item>
            </div>
          ) : null}
        </Form>
      </Modal>
    </section>
  );
}

function buildPayload(values: ChannelFormValues, editing: boolean) {
  const config: Record<string, unknown> = {};
  const secret: Record<string, unknown> = {};

  if (values.kind === "webhook") {
    if (values.webhook_url?.trim()) {
      secret.url = values.webhook_url.trim();
    }
  }

  if (values.kind === "email") {
    config.smtp_host = values.smtp_host?.trim();
    config.smtp_port = values.smtp_port;
    config.smtp_user = values.smtp_user?.trim();
    config.from = values.from?.trim();
    config.to = splitRecipients(values.to);
    if (values.smtp_password?.trim()) {
      secret.smtp_password = values.smtp_password.trim();
    }
  }

  if (values.kind === "pagerduty") {
    if (values.pagerduty_service?.trim()) {
      config.service = values.pagerduty_service.trim();
    }
    if (values.pagerduty_routing_key?.trim()) {
      secret.routing_key = values.pagerduty_routing_key.trim();
    }
  }

  return {
    name: values.name.trim(),
    enabled: values.enabled,
    config,
    ...(Object.keys(secret).length > 0 ? { secret } : {}),
    ...(editing ? {} : { kind: values.kind })
  };
}

function toFormValues(channel: NotificationChannel): ChannelFormValues {
  const config = channel.config ?? {};
  const to = config.to;

  return {
    name: channel.name,
    kind: channel.kind,
    enabled: channel.enabled,
    smtp_host: text(config.smtp_host),
    smtp_port: numberValue(config.smtp_port) ?? 587,
    smtp_user: text(config.smtp_user),
    from: text(config.from),
    to: Array.isArray(to) ? to.map(String).join(", ") : text(to),
    pagerduty_service: text(config.service)
  };
}

function splitRecipients(value?: string): string[] {
  return (value ?? "")
    .split(/[,\n;]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

function text(value: unknown): string | undefined {
  return typeof value === "string" ? value : undefined;
}

function numberValue(value: unknown): number | undefined {
  return typeof value === "number" ? value : undefined;
}

function targetSummary(config: Record<string, unknown>): string {
  const summary = text(config.target_summary);

  return summary ?? "-";
}

function KindTag({ kind }: { kind: NotificationChannelKind }) {
  const color =
    kind === "webhook" ? "purple" : kind === "email" ? "blue" : "orange";

  return <Tag color={color}>{kind}</Tag>;
}

function TestStatusTag({ status }: { status?: string | null }) {
  if (!status) {
    return <Tag>untested</Tag>;
  }

  return (
    <Tag color={status === "success" ? "green" : "red"}>{status}</Tag>
  );
}
