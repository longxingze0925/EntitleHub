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
  Tag,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Archive,
  Ban,
  Edit3,
  KeyRound,
  List,
  Pencil,
  Plus,
  Power,
  PowerOff,
  RefreshCw
} from "lucide-react";
import { useState } from "react";

import {
  createApplication,
  createServerApiKey,
  listApplicationSigningKeys,
  listApplications,
  listServerApiKeys,
  revokeServerApiKey,
  rotateApplicationKeys,
  updateServerApiKey,
  updateApplication,
  type ApplicationSummary,
  type CreateApplicationPayload,
  type CreateApplicationResult,
  type RotateApplicationKeysResult,
  type ServerApiKey,
  type SigningKeySummary,
  type UpdateApplicationPayload
} from "../../api/admin";
import { ConfirmActionButton } from "../../components/ConfirmActionButton";
import { HistoryToggle } from "../../components/HistoryToggle";
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

interface ServerApiKeyFormValues {
  name: string;
}

interface ServerApiKeyEditFormValues {
  name: string;
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
  const queryClient = useQueryClient();
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string | undefined>();
  const [includeHistory, setIncludeHistory] = useState(false);
  const [page, setPage] = useState(1);
  const [createOpen, setCreateOpen] = useState(false);
  const [editingApp, setEditingApp] = useState<ApplicationSummary | null>(null);
  const [keyTarget, setKeyTarget] = useState<ApplicationSummary | null>(null);
  const [serverKeyTarget, setServerKeyTarget] = useState<ApplicationSummary | null>(null);
  const [selectedSigningKey, setSelectedSigningKey] =
    useState<SigningKeySummary | null>(null);
  const [secretResult, setSecretResult] = useState<
    CreateApplicationResult | RotateApplicationKeysResult | null
  >(null);
  const [serverApiKeyModalOpen, setServerApiKeyModalOpen] = useState(false);
  const [serverApiKeyEditModalOpen, setServerApiKeyEditModalOpen] = useState(false);
  const [generatedServerApiKey, setGeneratedServerApiKey] = useState<string | null>(null);
  const [editingServerApiKey, setEditingServerApiKey] = useState<ServerApiKey | null>(null);
  const [createForm] = Form.useForm<ApplicationFormValues>();
  const [editForm] = Form.useForm<ApplicationFormValues>();
  const [serverApiKeyForm] = Form.useForm<ServerApiKeyFormValues>();
  const [serverApiKeyEditForm] = Form.useForm<ServerApiKeyEditFormValues>();
  const permissions = useAuthStore((state) => state.permissions);
  const canCreate = hasPermission(permissions, "app:create");
  const canUpdate = hasPermission(permissions, "app:update");
  const canReadKey = hasPermission(permissions, "app:read_key");
  const canRotateKey = hasPermission(permissions, "app:rotate_key");
  const canReadServerApiKey = hasPermission(permissions, "server_api_key:read");
  const canUpdateServerApiKey = hasPermission(permissions, "server_api_key:update");
  const showServerApiKeyManagement = canReadServerApiKey || canUpdateServerApiKey;
  const query = useQuery({
    queryKey: ["admin", "apps", keyword, status, includeHistory, page],
    queryFn: () =>
      listApplications({
        keyword,
        status,
        include_history: includeHistory,
        page,
        page_size: pageSize
      })
  });
  const keysQuery = useQuery({
    queryKey: ["admin", "app-signing-keys", keyTarget?.id],
    queryFn: () => listApplicationSigningKeys(keyTarget!.id),
    enabled: Boolean(keyTarget)
  });
  const serverApiKeysQuery = useQuery({
    queryKey: ["admin", "server-api-keys", serverKeyTarget?.id, includeHistory],
    queryFn: () =>
      listServerApiKeys({
        app_id: serverKeyTarget!.id,
        include_history: includeHistory
      }),
    enabled: Boolean(serverKeyTarget) && showServerApiKeyManagement
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
  const serverApiKeyMutation = useMutation({
    mutationFn: (values: ServerApiKeyFormValues) => {
      if (!serverKeyTarget) {
        throw new Error("application not selected");
      }

      return createServerApiKey({
        app_id: serverKeyTarget.id,
        name: values.name.trim(),
        scopes: ["ai:invoke"]
      });
    },
    onSuccess: async (result) => {
      message.success("服务端 Key 已生成");
      setGeneratedServerApiKey(result.plain_key);
      await queryClient.invalidateQueries({ queryKey: ["admin", "server-api-keys"] });
    }
  });
  const updateServerApiKeyMutation = useMutation({
    mutationFn: (values: ServerApiKeyEditFormValues) => {
      if (!editingServerApiKey) {
        throw new Error("server api key not selected");
      }

      return updateServerApiKey({
        id: editingServerApiKey.id,
        payload: {
          name: values.name.trim(),
          scopes: ["ai:invoke"]
        }
      });
    },
    onSuccess: async () => {
      message.success("服务端 Key 已更新");
      setServerApiKeyEditModalOpen(false);
      setEditingServerApiKey(null);
      serverApiKeyEditForm.resetFields();
      await queryClient.invalidateQueries({ queryKey: ["admin", "server-api-keys"] });
    }
  });
  const revokeServerApiKeyMutation = useMutation({
    mutationFn: (id: string) => revokeServerApiKey(id),
    onSuccess: async () => {
      message.success("服务端 Key 已吊销");
      await queryClient.invalidateQueries({ queryKey: ["admin", "server-api-keys"] });
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
      width: 540,
      render: (_, record) => (
        <Space>
          {canUpdate ? (
            <Button size="small" icon={<Edit3 size={14} />} onClick={() => openEdit(record)}>
              编辑
            </Button>
          ) : null}
          {canUpdate && record.status !== "active" ? (
            <Popconfirm
              title="启用应用"
              onConfirm={() => updateApplicationStatus(record, "active")}
            >
              <Button
                size="small"
                icon={<Power size={14} />}
                loading={updateMutation.isPending}
              >
                启用
              </Button>
            </Popconfirm>
          ) : null}
          {canUpdate && record.status === "active" ? (
            <Popconfirm
              title="禁用应用"
              onConfirm={() => updateApplicationStatus(record, "disabled")}
            >
              <Button
                size="small"
                icon={<PowerOff size={14} />}
                loading={updateMutation.isPending}
              >
                禁用
              </Button>
            </Popconfirm>
          ) : null}
          {canUpdate && record.status !== "archived" ? (
            <ConfirmActionButton
              title="归档应用"
              description="归档后默认列表不再显示该应用。"
              buttonProps={{
                danger: true,
                size: "small",
                icon: <Archive size={14} />
              }}
              loading={updateMutation.isPending}
              onConfirm={() => updateApplicationStatus(record, "archived")}
            >
              归档
            </ConfirmActionButton>
          ) : null}
          {canReadKey ? (
            <Button size="small" icon={<List size={14} />} onClick={() => setKeyTarget(record)}>
              密钥
            </Button>
          ) : null}
          {showServerApiKeyManagement ? (
            <Button
              size="small"
              icon={<KeyRound size={14} />}
              onClick={() => setServerKeyTarget(record)}
            >
              服务端 Key
            </Button>
          ) : null}
          {canRotateKey ? (
            <ConfirmActionButton
              title="轮换密钥"
              description="轮换后会生成新的应用密钥和签名密钥，旧密钥进入历史状态。"
              buttonProps={{
                size: "small",
                icon: <KeyRound size={14} />
              }}
              loading={rotateMutation.isPending}
              onConfirm={() => rotateMutation.mutate(record.id)}
            >
              轮换
            </ConfirmActionButton>
          ) : null}
        </Space>
      )
    }
  ];

  const keyColumns: ColumnsType<SigningKeySummary> = [
    {
      title: "KID",
      dataIndex: "kid",
      key: "kid",
      render: (value: string) => (
        <Typography.Text copyable ellipsis>
          {value}
        </Typography.Text>
      )
    },
    {
      title: "用途",
      dataIndex: "key_scope",
      key: "key_scope",
      width: 130,
      render: (value: string) => signingKeyScopeLabel(value)
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
    },
    {
      title: "操作",
      key: "actions",
      width: 100,
      render: (_, record) => (
        <Button
          size="small"
          icon={<KeyRound size={14} />}
          onClick={() => setSelectedSigningKey(record)}
        >
          公钥
        </Button>
      )
    }
  ];

  const serverApiKeyColumns: ColumnsType<ServerApiKey> = [
    {
      title: "名称",
      dataIndex: "name",
      key: "name",
      width: 220,
      render: (value: string) => (
        <Typography.Text ellipsis title={value}>
          {value}
        </Typography.Text>
      )
    },
    {
      title: "Key 前缀",
      dataIndex: "key_prefix",
      key: "key_prefix",
      width: 180,
      render: (value: string) => <Typography.Text code>{value}</Typography.Text>
    },
    {
      title: "权限",
      dataIndex: "scopes",
      key: "scopes",
      width: 130,
      render: (value: string[]) => (
        <Space size={4} wrap>
          {value.map((scope) => (
            <Tag key={scope}>{scopeLabel(scope)}</Tag>
          ))}
        </Space>
      )
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 100,
      render: (value: string) => (
        <Tag color={value === "active" ? "green" : "default"}>
          {value === "active" ? "启用" : "已吊销"}
        </Tag>
      )
    },
    {
      title: "最近使用",
      dataIndex: "last_used_at",
      key: "last_used_at",
      width: 180,
      render: (value?: string | null) => (value ? dateTime(value) : "-")
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
      width: 150,
      render: (_, record) => (
        <Space size={6}>
          <Button
            aria-label={`编辑服务端 Key ${record.name}`}
            size="small"
            icon={<Pencil size={14} />}
            disabled={!canUpdateServerApiKey || record.status !== "active"}
            onClick={() => openEditServerApiKey(record)}
          >
            编辑
          </Button>
          {record.status === "active" ? (
            <ConfirmActionButton
              title="吊销服务端 Key"
              description="吊销后，使用这个 Key 的服务端应用将无法继续调用 AI 接口。"
              confirmText="吊销"
              okText="吊销"
              loading={revokeServerApiKeyMutation.isPending}
              buttonProps={{
                size: "small",
                danger: true,
                disabled: !canUpdateServerApiKey,
                icon: <Ban size={14} />
              }}
              onConfirm={() => revokeServerApiKeyMutation.mutate(record.id)}
            >
              吊销
            </ConfirmActionButton>
          ) : null}
        </Space>
      )
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

  const updateApplicationStatus = (app: ApplicationSummary, nextStatus: string) => {
    updateMutation.mutate({
      id: app.id,
      payload: {
        status: nextStatus
      }
    });
  };

  const openCreateServerApiKey = () => {
    setGeneratedServerApiKey(null);
    serverApiKeyForm.setFieldsValue({
      name: "生产服务端 Key"
    });
    setServerApiKeyModalOpen(true);
  };

  const openEditServerApiKey = (serverApiKey: ServerApiKey) => {
    setEditingServerApiKey(serverApiKey);
    serverApiKeyEditForm.setFieldsValue({
      name: serverApiKey.name
    });
    setServerApiKeyEditModalOpen(true);
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
            value={status}
            options={statusOptions}
            onChange={(value) => {
              setPage(1);
              setStatus(value);
            }}
          />
          <HistoryToggle
            checked={includeHistory}
            onChange={(checked) => {
              setPage(1);
              setIncludeHistory(checked);
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
      {serverApiKeysQuery.error ||
      serverApiKeyMutation.error ||
      updateServerApiKeyMutation.error ||
      revokeServerApiKeyMutation.error ? (
        <Alert type="error" message="服务端 Key 操作失败，请稍后重试" />
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
        width={760}
      >
        <Space direction="vertical" size={12} className="token-result">
          <Alert
            type="info"
            showIcon
            message="应用密钥只会在创建或轮换时显示一次；签名公钥可以公开复制给客户端用于验签。"
          />
          <Typography.Text type="secondary">应用 Key（app_key）</Typography.Text>
          <Typography.Text copyable>{secretResult?.app_key}</Typography.Text>
          <Typography.Text type="secondary">应用密钥（app_secret）</Typography.Text>
          <Typography.Text copyable>{secretResult?.app_secret}</Typography.Text>
          <Typography.Text type="secondary">签名密钥 ID（signing_kid）</Typography.Text>
          <Typography.Text copyable>{secretResult?.signing_key.kid}</Typography.Text>
          <Typography.Text type="secondary">公开 JWKS 地址</Typography.Text>
          <Typography.Text
            copyable={{
              text: secretResult ? applicationJwksUrl(secretResult.app_key) : ""
            }}
          >
            {secretResult ? applicationJwksUrl(secretResult.app_key) : "-"}
          </Typography.Text>
          <Typography.Text type="secondary">签名公钥（public_key_pem）</Typography.Text>
          <Typography.Paragraph
            copyable={{ text: secretResult?.signing_key.public_key_pem ?? "" }}
            className="public-key-block"
          >
            {secretResult?.signing_key.public_key_pem}
          </Typography.Paragraph>
        </Space>
      </Modal>

      <Modal
        title={keyTarget ? `签名密钥：${keyTarget.name}` : "签名密钥"}
        open={Boolean(keyTarget)}
        onCancel={() => {
          setKeyTarget(null);
          setSelectedSigningKey(null);
        }}
        onOk={() => {
          setKeyTarget(null);
          setSelectedSigningKey(null);
        }}
        width={860}
      >
        {keyTarget ? (
          <Alert
            type="info"
            showIcon
            message="客户端建议使用公开 JWKS 地址自动获取签名公钥。"
            description={
              <Typography.Text
                copyable={{ text: applicationJwksUrl(keyTarget.app_key) }}
              >
                {applicationJwksUrl(keyTarget.app_key)}
              </Typography.Text>
            }
            style={{ marginBottom: 12 }}
          />
        ) : null}
        <Table
          rowKey="id"
          loading={keysQuery.isLoading}
          columns={keyColumns}
          dataSource={keysQuery.data?.items ?? []}
          pagination={false}
          locale={{ emptyText: "暂无数据" }}
        />
      </Modal>

      <Modal
        title={serverKeyTarget ? `服务端 Key：${serverKeyTarget.name}` : "服务端 Key"}
        open={Boolean(serverKeyTarget)}
        onCancel={() => {
          setServerKeyTarget(null);
          setGeneratedServerApiKey(null);
          setServerApiKeyModalOpen(false);
          setServerApiKeyEditModalOpen(false);
          setEditingServerApiKey(null);
        }}
        footer={null}
        width={980}
      >
        <Space direction="vertical" size={12} className="settings-stack">
          <Alert
            type="info"
            showIcon
            message="服务端 Key 用于后端服务调用 `/api/server/ai/v1/*`，客户端和浏览器前端不应保存它。"
          />
          <div className="table-toolbar">
            <Button
              type="primary"
              icon={<KeyRound size={16} />}
              disabled={!canUpdateServerApiKey || serverKeyTarget?.status !== "active"}
              onClick={openCreateServerApiKey}
            >
              生成服务端 Key
            </Button>
          </div>
          <Table
            rowKey="id"
            loading={serverApiKeysQuery.isLoading}
            columns={serverApiKeyColumns}
            dataSource={serverApiKeysQuery.data?.items ?? []}
            pagination={false}
            scroll={{ x: "max-content" }}
            locale={{ emptyText: "暂无数据" }}
          />
        </Space>
      </Modal>

      <Modal
        title="签名公钥"
        open={Boolean(selectedSigningKey)}
        onCancel={() => setSelectedSigningKey(null)}
        onOk={() => setSelectedSigningKey(null)}
        width={760}
      >
        <Space direction="vertical" size={12} className="token-result">
          <Typography.Text type="secondary">签名密钥 ID（KID）</Typography.Text>
          <Typography.Text copyable>{selectedSigningKey?.kid}</Typography.Text>
          <Typography.Text type="secondary">用途</Typography.Text>
          <Typography.Text>
            {selectedSigningKey
              ? signingKeyScopeLabel(selectedSigningKey.key_scope)
              : "-"}
          </Typography.Text>
          {keyTarget ? (
            <>
              <Typography.Text type="secondary">公开 JWKS 地址</Typography.Text>
              <Typography.Text
                copyable={{ text: applicationJwksUrl(keyTarget.app_key) }}
              >
                {applicationJwksUrl(keyTarget.app_key)}
              </Typography.Text>
            </>
          ) : null}
          <Typography.Text type="secondary">签名公钥（public_key_pem）</Typography.Text>
          <Typography.Paragraph
            copyable={{
              text: selectedSigningKey?.public_key_pem ?? ""
            }}
            className="public-key-block"
          >
            {selectedSigningKey?.public_key_pem}
          </Typography.Paragraph>
        </Space>
      </Modal>

      <Modal
        title={serverKeyTarget ? `生成服务端 Key：${serverKeyTarget.name}` : "生成服务端 Key"}
        open={serverApiKeyModalOpen}
        onCancel={() => {
          setServerApiKeyModalOpen(false);
          setGeneratedServerApiKey(null);
          serverApiKeyForm.resetFields();
        }}
        onOk={() => serverApiKeyForm.submit()}
        okButtonProps={{ disabled: Boolean(generatedServerApiKey) }}
        confirmLoading={serverApiKeyMutation.isPending}
        destroyOnHidden
      >
        <Space direction="vertical" size={12} className="settings-stack">
          {generatedServerApiKey ? (
            <Alert
              type="success"
              showIcon
              message="请立即复制保存，关闭后不会再次显示明文 Key。"
              description={
                <Typography.Paragraph copyable className="api-key-preview">
                  {generatedServerApiKey}
                </Typography.Paragraph>
              }
            />
          ) : null}
          <Form<ServerApiKeyFormValues>
            form={serverApiKeyForm}
            layout="vertical"
            onFinish={(values) => serverApiKeyMutation.mutate(values)}
          >
            <Form.Item name="name" label="名称" rules={[{ required: true }]}>
              <Input placeholder="例如：影织生产服务端" />
            </Form.Item>
            <Typography.Text type="secondary">
              当前 Key 固定绑定到这个应用，并仅允许服务端发起 AI 调用。
            </Typography.Text>
          </Form>
        </Space>
      </Modal>

      <Modal
        title={
          editingServerApiKey
            ? `编辑服务端 Key：${editingServerApiKey.key_prefix}`
            : "编辑服务端 Key"
        }
        open={serverApiKeyEditModalOpen}
        onCancel={() => {
          setServerApiKeyEditModalOpen(false);
          setEditingServerApiKey(null);
        }}
        onOk={() => serverApiKeyEditForm.submit()}
        confirmLoading={updateServerApiKeyMutation.isPending}
        destroyOnHidden
      >
        <Form<ServerApiKeyEditFormValues>
          form={serverApiKeyEditForm}
          layout="vertical"
          onFinish={(values) => updateServerApiKeyMutation.mutate(values)}
        >
          <Form.Item name="name" label="名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Typography.Text type="secondary">
            当前权限固定为 AI 调用。需要停用时请直接吊销并重新生成。
          </Typography.Text>
        </Form>
      </Modal>
    </section>
  );
}

function applicationJwksUrl(appKey: string): string {
  return `${window.location.origin}/api/client/apps/${encodeURIComponent(appKey)}/jwks`;
}

function signingKeyScopeLabel(scope: string): string {
  const labels: Record<string, string> = {
    app_request: "应用请求",
    release_file: "版本文件",
    secure_script: "安全脚本"
  };

  return labels[scope] ?? scope;
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

function scopeLabel(value: string): string {
  const labels: Record<string, string> = {
    "ai:invoke": "AI 调用"
  };

  return labels[value] ?? value;
}
