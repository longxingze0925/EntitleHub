import {
  Alert,
  Button,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Table,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Edit3, KeyRound, List, Plus, RefreshCw } from "lucide-react";
import { useState } from "react";

import {
  createApplication,
  listApplicationSigningKeys,
  listApplications,
  rotateApplicationKeys,
  updateApplication,
  type ApplicationSummary,
  type CreateApplicationPayload,
  type CreateApplicationResult,
  type RotateApplicationKeysResult,
  type SigningKeySummary,
  type UpdateApplicationPayload
} from "../../api/admin";
import { SimplePager } from "../../components/SimplePager";
import { StatusTag } from "../../components/StatusTag";
import { useAuthStore } from "../../stores/authStore";
import { dateTime, shortId } from "../../utils/format";
import { tMessage, tOption, tStatus } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

const pageSize = 20;

interface ApplicationFormValues {
  name: string;
  slug?: string;
  auth_mode?: string;
  status?: string;
  heartbeat_interval_seconds?: number | null;
  offline_tolerance_seconds?: number | null;
  max_devices_default?: number | null;
}

const authModeOptions = [
  tOption("both"),
  tOption("license"),
  tOption("subscription")
];

const statusOptions = [
  tOption("active"),
  tOption("disabled"),
  tOption("archived")
];

export function ApplicationsPage() {
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string | undefined>();
  const [page, setPage] = useState(1);
  const [createOpen, setCreateOpen] = useState(false);
  const [editingApp, setEditingApp] = useState<ApplicationSummary | null>(null);
  const [keyTarget, setKeyTarget] = useState<ApplicationSummary | null>(null);
  const [secretResult, setSecretResult] = useState<
    CreateApplicationResult | RotateApplicationKeysResult | null
  >(null);
  const [createForm] = Form.useForm<ApplicationFormValues>();
  const [editForm] = Form.useForm<ApplicationFormValues>();
  const permissions = useAuthStore((state) => state.permissions);
  const canCreate = hasPermission(permissions, "app:create");
  const canUpdate = hasPermission(permissions, "app:update");
  const canReadKey = hasPermission(permissions, "app:read_key");
  const canRotateKey = hasPermission(permissions, "app:rotate_key");
  const query = useQuery({
    queryKey: ["admin", "apps", keyword, status, page],
    queryFn: () => listApplications({ keyword, status, page, page_size: pageSize })
  });
  const keysQuery = useQuery({
    queryKey: ["admin", "app-signing-keys", keyTarget?.id],
    queryFn: () => listApplicationSigningKeys(keyTarget!.id),
    enabled: Boolean(keyTarget)
  });
  const createMutation = useMutation({
    mutationFn: createApplication,
    onSuccess: async (data) => {
      message.success(tMessage("application_created"));
      setSecretResult(data);
      setCreateOpen(false);
      createForm.resetFields();
      await query.refetch();
    }
  });
  const updateMutation = useMutation({
    mutationFn: updateApplication,
    onSuccess: async () => {
      message.success(tMessage("application_updated"));
      setEditingApp(null);
      editForm.resetFields();
      await query.refetch();
    }
  });
  const rotateMutation = useMutation({
    mutationFn: rotateApplicationKeys,
    onSuccess: async (data) => {
      message.success(tMessage("application_keys_rotated"));
      setSecretResult(data);
      await query.refetch();
      await keysQuery.refetch();
    }
  });

  const columns: ColumnsType<ApplicationSummary> = [
    {
      title: "应用",
      dataIndex: "name",
      key: "name",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text strong>{record.name}</Typography.Text>
          <Typography.Text type="secondary">
            {record.slug ?? shortId(record.id)}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "应用 Key",
      dataIndex: "app_key",
      key: "app_key"
    },
    {
      title: "认证",
      dataIndex: "auth_mode",
      key: "auth_mode",
      width: 130,
      render: (value: string) => tStatus(value)
    },
    {
      title: "设备默认上限",
      dataIndex: "max_devices_default",
      key: "max_devices_default",
      width: 130
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 110,
      render: (value) => <StatusTag value={value} />
    },
    {
      title: "操作",
      key: "actions",
      width: 260,
      render: (_, record) => (
        <Space>
          {canUpdate ? (
            <Button size="small" icon={<Edit3 size={14} />} onClick={() => openEdit(record)}>
              编辑
            </Button>
          ) : null}
          {canReadKey ? (
            <Button size="small" icon={<List size={14} />} onClick={() => setKeyTarget(record)}>
              密钥
            </Button>
          ) : null}
          {canRotateKey ? (
            <Popconfirm
              title="轮换密钥"
              onConfirm={() => rotateMutation.mutate(record.id)}
            >
              <Button
                size="small"
                icon={<KeyRound size={14} />}
                loading={rotateMutation.isPending}
              >
                轮换
              </Button>
            </Popconfirm>
          ) : null}
        </Space>
      )
    }
  ];

  const keyColumns: ColumnsType<SigningKeySummary> = [
    {
      title: "KID",
      dataIndex: "kid",
      key: "kid"
    },
    {
      title: "范围",
      dataIndex: "key_scope",
      key: "key_scope",
      width: 130
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 110,
      render: (value) => <StatusTag value={value} />
    },
    {
      title: "创建时间",
      dataIndex: "created_at",
      key: "created_at",
      render: (value: string) => dateTime(value)
    }
  ];

  const openEdit = (app: ApplicationSummary) => {
    setEditingApp(app);
    editForm.setFieldsValue({
      name: app.name,
      slug: app.slug ?? undefined,
      auth_mode: app.auth_mode,
      status: app.status,
      heartbeat_interval_seconds: app.heartbeat_interval_seconds,
      offline_tolerance_seconds: app.offline_tolerance_seconds,
      max_devices_default: app.max_devices_default
    });
  };

  const submitCreate = (values: ApplicationFormValues) => {
    const payload: CreateApplicationPayload = {
      name: values.name.trim(),
      slug: clean(values.slug),
      auth_mode: values.auth_mode,
      heartbeat_interval_seconds: cleanNumber(values.heartbeat_interval_seconds),
      offline_tolerance_seconds: cleanNumber(values.offline_tolerance_seconds),
      max_devices_default: cleanNumber(values.max_devices_default)
    };
    createMutation.mutate(payload);
  };

  const submitUpdate = (values: ApplicationFormValues) => {
    if (!editingApp) {
      return;
    }

    const payload: UpdateApplicationPayload = {
      name: clean(values.name),
      slug: clean(values.slug),
      auth_mode: values.auth_mode,
      status: values.status,
      heartbeat_interval_seconds: cleanNumber(values.heartbeat_interval_seconds),
      offline_tolerance_seconds: cleanNumber(values.offline_tolerance_seconds),
      max_devices_default: cleanNumber(values.max_devices_default)
    };
    updateMutation.mutate({ id: editingApp.id, payload });
  };

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>应用管理</Typography.Title>
          <Typography.Text type="secondary">应用配置、密钥轮换和签名公钥</Typography.Text>
        </div>
        <Space>
          <Input.Search
            allowClear
            placeholder="关键词"
            onSearch={(value) => {
              setPage(1);
              setKeyword(value);
            }}
            className="table-search"
          />
          <Select
            allowClear
            placeholder="状态"
            className="table-filter"
            options={statusOptions}
            onChange={(value) => {
              setPage(1);
              setStatus(value);
            }}
          />
          <Button icon={<RefreshCw size={16} />} onClick={() => query.refetch()} />
          {canCreate ? (
            <Button
              type="primary"
              icon={<Plus size={16} />}
              onClick={() => setCreateOpen(true)}
            >
              创建应用
            </Button>
          ) : null}
        </Space>
      </div>
      {query.error ? (
        <Alert type="error" message={tMessage("applications_load_failed")} />
      ) : null}
      {createMutation.error ? (
        <Alert type="error" message={tMessage("application_create_failed")} />
      ) : null}
      {updateMutation.error ? (
        <Alert type="error" message={tMessage("application_update_failed")} />
      ) : null}
      {rotateMutation.error ? (
        <Alert type="error" message={tMessage("application_rotate_keys_failed")} />
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

      <ApplicationFormModal
        title="创建应用"
        open={createOpen}
        form={createForm}
        loading={createMutation.isPending}
        onCancel={() => setCreateOpen(false)}
        onSubmit={submitCreate}
        includeStatus={false}
      />

      <ApplicationFormModal
        title="编辑应用"
        open={Boolean(editingApp)}
        form={editForm}
        loading={updateMutation.isPending}
        onCancel={() => {
          setEditingApp(null);
          editForm.resetFields();
        }}
        onSubmit={submitUpdate}
        includeStatus
      />

      <Modal
        title="应用密钥"
        open={Boolean(secretResult)}
        onCancel={() => setSecretResult(null)}
        onOk={() => setSecretResult(null)}
      >
        <Space direction="vertical" size={12} className="token-result">
          <Typography.Text type="secondary">应用 Key（app_key）</Typography.Text>
          <Typography.Text copyable>{secretResult?.app_key}</Typography.Text>
          <Typography.Text type="secondary">应用密钥（app_secret）</Typography.Text>
          <Typography.Text copyable>{secretResult?.app_secret}</Typography.Text>
          <Typography.Text type="secondary">签名密钥 ID（signing_kid）</Typography.Text>
          <Typography.Text copyable>{secretResult?.signing_key.kid}</Typography.Text>
        </Space>
      </Modal>

      <Modal
        title="签名密钥"
        open={Boolean(keyTarget)}
        onCancel={() => setKeyTarget(null)}
        onOk={() => setKeyTarget(null)}
        width={760}
      >
        <Table
          rowKey="id"
          loading={keysQuery.isLoading}
          columns={keyColumns}
          dataSource={keysQuery.data?.items ?? []}
          pagination={false}
          locale={{ emptyText: "暂无数据" }}
        />
      </Modal>
    </section>
  );
}

function ApplicationFormModal(props: {
  title: string;
  open: boolean;
  form: ReturnType<typeof Form.useForm<ApplicationFormValues>>[0];
  loading: boolean;
  onCancel: () => void;
  onSubmit: (values: ApplicationFormValues) => void;
  includeStatus: boolean;
}) {
  return (
    <Modal
      title={props.title}
      open={props.open}
      onCancel={props.onCancel}
      onOk={() => props.form.submit()}
      confirmLoading={props.loading}
      destroyOnClose
    >
      <Form<ApplicationFormValues>
        form={props.form}
        layout="vertical"
        onFinish={props.onSubmit}
        initialValues={{
          auth_mode: "both",
          heartbeat_interval_seconds: 3600,
          offline_tolerance_seconds: 86400,
          max_devices_default: 1
        }}
      >
        <Form.Item
          name="name"
          label="应用名称"
          rules={[{ required: true, message: "请输入应用名称" }]}
        >
          <Input />
        </Form.Item>
        <Form.Item
          name="slug"
          label="应用标识（Slug）"
          rules={[
            {
              pattern: /^[a-z0-9-]+$/,
              message: "只能使用小写字母、数字和连字符"
            }
          ]}
        >
          <Input />
        </Form.Item>
        <Form.Item name="auth_mode" label="认证模式">
          <Select options={authModeOptions} />
        </Form.Item>
        {props.includeStatus ? (
          <Form.Item name="status" label="状态">
            <Select options={statusOptions} />
          </Form.Item>
        ) : null}
        <Form.Item name="max_devices_default" label="默认设备上限">
          <InputNumber min={0} precision={0} className="form-number" />
        </Form.Item>
        <Form.Item name="heartbeat_interval_seconds" label="心跳间隔（秒）">
          <InputNumber min={1} precision={0} className="form-number" />
        </Form.Item>
        <Form.Item name="offline_tolerance_seconds" label="离线容忍（秒）">
          <InputNumber min={1} precision={0} className="form-number" />
        </Form.Item>
      </Form>
    </Modal>
  );
}

function clean(value?: string): string | undefined {
  const trimmed = value?.trim();

  return trimmed ? trimmed : undefined;
}

function cleanNumber(value?: number | null): number | undefined {
  return typeof value === "number" ? value : undefined;
}
