import {
  Alert,
  Button,
  Divider,
  Form,
  Input,
  QRCode,
  Segmented,
  Space,
  Table,
  Tag,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import {
  KeyRound,
  LogOut,
  MailCheck,
  MonitorCheck,
  RefreshCw,
  ShieldCheck
} from "lucide-react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useMemo, useState } from "react";

import {
  listAdminSessions,
  listGlobalJwtSigningKeys,
  revokeAdminSession,
  rotateGlobalJwtSigningKey,
  type AdminSessionSummary,
  type SigningKeySummary
} from "../../api/admin";
import {
  changePassword,
  disableMfa,
  enableMfa,
  regenerateMfaRecoveryCodes,
  requestEmailVerify,
  setupMfa,
  type MfaSetupResult
} from "../../api/auth";
import { ConfirmActionButton } from "../../components/ConfirmActionButton";
import { StatusTag } from "../../components/StatusTag";
import { useAuthStore } from "../../stores/authStore";
import { dateTime } from "../../utils/format";
import { tMessage, tStatus } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";
import { requiresMfaForRole } from "../../utils/security";

interface PasswordFormValues {
  old_password: string;
  new_password: string;
}

interface MfaCodeFormValues {
  code: string;
}

interface MfaProtectedFormValues {
  password: string;
  code: string;
}

type SessionStatusFilter = "active" | "all" | "expired" | "revoked";

const sessionStatusColor: Record<string, string> = {
  active: "green",
  expired: "default",
  revoked: "red"
};

const sessionStatusText: Record<string, string> = {
  active: "有效",
  expired: "已过期",
  revoked: "已撤销"
};

const sessionStatusFilterOptions: Array<{
  label: string;
  value: SessionStatusFilter;
}> = [
  { label: "有效", value: "active" },
  { label: "全部", value: "all" },
  { label: "已过期", value: "expired" },
  { label: "已撤销", value: "revoked" }
];

export function SecurityPage() {
  const { user, tenant, roles, permissions, setProfile } = useAuthStore();
  const [passwordForm] = Form.useForm<PasswordFormValues>();
  const [enableForm] = Form.useForm<MfaCodeFormValues>();
  const [disableForm] = Form.useForm<MfaProtectedFormValues>();
  const [regenerateForm] = Form.useForm<MfaProtectedFormValues>();
  const [setupResult, setSetupResult] = useState<MfaSetupResult | null>(null);
  const [recoveryCodes, setRecoveryCodes] = useState<string[]>([]);
  const [revokingSessionId, setRevokingSessionId] = useState<string | null>(
    null
  );
  const [sessionStatusFilter, setSessionStatusFilter] =
    useState<SessionStatusFilter>("active");
  const canReadSecurity = hasPermission(permissions, "security:read");
  const canRotateSecurityKey = hasPermission(permissions, "security:rotate_key");
  const mfaRequired = requiresMfaForRole(roles);

  const jwtKeysQuery = useQuery({
    queryKey: ["admin", "global-jwt-signing-keys"],
    queryFn: listGlobalJwtSigningKeys,
    enabled: canReadSecurity
  });

  const adminSessionsQuery = useQuery({
    queryKey: ["admin", "sessions"],
    queryFn: listAdminSessions,
    enabled: Boolean(user)
  });
  const filteredAdminSessions = useMemo(() => {
    const items = adminSessionsQuery.data?.items ?? [];
    if (sessionStatusFilter === "all") {
      return items;
    }

    return items.filter((session) => session.status === sessionStatusFilter);
  }, [adminSessionsQuery.data?.items, sessionStatusFilter]);

  const updateCurrentUser = (patch: Partial<NonNullable<typeof user>>) => {
    if (!user || !tenant) {
      return;
    }
    setProfile({
      user: { ...user, ...patch },
      tenant,
      roles,
      permissions
    });
  };

  const passwordMutation = useMutation({
    mutationFn: changePassword,
    onSuccess: (data) => {
      passwordForm.resetFields();
      message.success(tMessage(`password_changed:${data.revoked_sessions}`));
    }
  });

  const emailMutation = useMutation({
    mutationFn: requestEmailVerify,
    onSuccess: () => {
      message.success(tMessage("email_verify_requested"));
    }
  });

  const setupMutation = useMutation({
    mutationFn: setupMfa,
    onSuccess: (data) => {
      setSetupResult(data);
      setRecoveryCodes(data.recovery_codes);
      enableForm.resetFields();
    }
  });

  const enableMutation = useMutation({
    mutationFn: (values: MfaCodeFormValues) => enableMfa(values.code.trim()),
    onSuccess: () => {
      setSetupResult(null);
      setRecoveryCodes([]);
      enableForm.resetFields();
      updateCurrentUser({ mfa_enabled: true });
      message.success(tMessage("mfa_enabled"));
    }
  });

  const disableMutation = useMutation({
    mutationFn: (values: MfaProtectedFormValues) =>
      disableMfa({
        password: values.password,
        code: values.code.trim()
      }),
    onSuccess: () => {
      disableForm.resetFields();
      updateCurrentUser({ mfa_enabled: false });
      message.success(tMessage("mfa_disabled"));
    }
  });

  const regenerateMutation = useMutation({
    mutationFn: (values: MfaProtectedFormValues) =>
      regenerateMfaRecoveryCodes({
        password: values.password,
        code: values.code.trim()
      }),
    onSuccess: (data) => {
      regenerateForm.resetFields();
      setRecoveryCodes(data.recovery_codes);
      message.success(tMessage("recovery_codes_regenerated"));
    }
  });

  const rotateJwtKeyMutation = useMutation({
    mutationFn: rotateGlobalJwtSigningKey,
    onSuccess: async (data) => {
      message.success(tMessage(`jwt_key_rotated:${data.retired_key_count}`));
      await jwtKeysQuery.refetch();
    }
  });

  const revokeSessionMutation = useMutation({
    mutationFn: revokeAdminSession,
    onMutate: (id) => {
      setRevokingSessionId(id);
    },
    onSuccess: async (data) => {
      message.success(
        `后台会话已撤销，已撤销 ${data.revoked_refresh_tokens} 个刷新令牌`
      );
      await adminSessionsQuery.refetch();
    },
    onSettled: () => {
      setRevokingSessionId(null);
    }
  });

  const adminSessionColumns: ColumnsType<AdminSessionSummary> = [
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 130,
      render: (value: string, record) => (
        <Space size={6} wrap>
          <Tag color={sessionStatusColor[value] ?? "blue"}>
            {sessionStatusText[value] ?? value}
          </Tag>
          {record.current ? <Tag color="blue">当前</Tag> : null}
        </Space>
      )
    },
    {
      title: "IP",
      dataIndex: "ip",
      key: "ip",
      width: 150,
      render: (value?: string | null) => value ?? "-"
    },
    {
      title: "设备信息",
      dataIndex: "user_agent",
      key: "user_agent",
      ellipsis: true,
      render: (value?: string | null) => value ?? "-"
    },
    {
      title: "最近活动",
      dataIndex: "last_seen_at",
      key: "last_seen_at",
      width: 180,
      render: (value: string | null | undefined, record) =>
        dateTime(value ?? record.created_at)
    },
    {
      title: "过期时间",
      dataIndex: "expires_at",
      key: "expires_at",
      width: 180,
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      fixed: "right",
      width: 120,
      render: (_, record) =>
        !record.current && record.status === "active" ? (
          <ConfirmActionButton
            title="撤销后台会话"
            description="该会话会立即失效，对应浏览器需要重新登录。"
            buttonProps={{
              danger: true,
              size: "small",
              icon: <LogOut size={14} />
            }}
            loading={revokingSessionId === record.id}
            onConfirm={() => revokeSessionMutation.mutate(record.id)}
          >
            踢下线
          </ConfirmActionButton>
        ) : (
          "-"
        )
    }
  ];

  const jwtKeyColumns: ColumnsType<SigningKeySummary> = [
    {
      title: "KID",
      dataIndex: "kid",
      key: "kid",
      render: (value: string) => <Typography.Text copyable>{value}</Typography.Text>
    },
    {
      title: "范围",
      dataIndex: "key_scope",
      key: "key_scope",
      width: 160
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 110,
      render: (value) => <StatusTag value={value} />
    },
    {
      title: "启用时间",
      dataIndex: "not_before",
      key: "not_before",
      width: 180,
      render: (value: string) => dateTime(value)
    },
    {
      title: "截止时间",
      dataIndex: "not_after",
      key: "not_after",
      width: 180,
      render: (value?: string | null) => (value ? dateTime(value) : "-")
    }
  ];

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>安全状态</Typography.Title>
          <Typography.Text type="secondary">
            {user?.email ?? "-"}
          </Typography.Text>
        </div>
        <Space>
          <Tag color={user?.email_verified ? "green" : "default"}>
            {user?.email_verified ? "邮箱已验证" : "邮箱未验证"}
          </Tag>
          <Tag color={user?.mfa_enabled ? "green" : "default"}>
            {user?.mfa_enabled ? tStatus("enabled") : "未启用多因素认证"}
          </Tag>
        </Space>
      </div>

      {mfaRequired && !user?.mfa_enabled ? (
        <Alert
          type="warning"
          showIcon
          message="当前管理员角色需要启用多因素认证"
          description="完成绑定后才能继续访问其他后台页面。"
        />
      ) : null}

      <div className="settings-grid">
        <section className="settings-panel">
          <div className="settings-panel-title">
            <KeyRound size={18} />
            <Typography.Title level={3}>修改密码</Typography.Title>
          </div>
          <Form<PasswordFormValues>
            form={passwordForm}
            layout="vertical"
            onFinish={(values) => passwordMutation.mutate(values)}
          >
            <Form.Item
              name="old_password"
              label="当前密码"
              rules={[{ required: true, message: "请输入当前密码" }]}
            >
              <Input.Password autoComplete="current-password" />
            </Form.Item>
            <Form.Item
              name="new_password"
              label="新密码"
              rules={[{ required: true, message: "请输入新密码" }]}
            >
              <Input.Password autoComplete="new-password" />
            </Form.Item>
            <Button
              type="primary"
              htmlType="submit"
              loading={passwordMutation.isPending}
            >
              保存
            </Button>
          </Form>
        </section>

        <section className="settings-panel">
          <div className="settings-panel-title">
            <MailCheck size={18} />
            <Typography.Title level={3}>邮箱验证</Typography.Title>
          </div>
          <Space direction="vertical" size={12}>
            <Typography.Text copyable>{user?.email ?? "-"}</Typography.Text>
            <Button
              onClick={() => emailMutation.mutate()}
              disabled={Boolean(user?.email_verified)}
              loading={emailMutation.isPending}
            >
              发送验证
            </Button>
          </Space>
        </section>

        <section className="settings-panel settings-panel-wide">
          <div className="settings-panel-title">
            <ShieldCheck size={18} />
            <Typography.Title level={3}>多因素认证</Typography.Title>
          </div>

          {!user?.mfa_enabled ? (
            <Space direction="vertical" size={14} className="settings-stack">
              <Button
                icon={<ShieldCheck size={16} />}
                onClick={() => setupMutation.mutate()}
                loading={setupMutation.isPending}
              >
                初始化多因素认证
              </Button>

              {setupResult ? (
                <>
                  <Alert
                    type="warning"
                    message={tMessage("recovery_codes_only_shown_once")}
                    showIcon
                  />
                  <div className="mfa-setup-grid">
                    <div className="mfa-qr-panel">
                      <QRCode
                        value={setupResult.otpauth_url}
                        size={176}
                        bordered={false}
                      />
                      <Typography.Text type="secondary">
                        使用身份验证器扫描
                      </Typography.Text>
                    </div>
                    <div className="secret-list">
                      <Typography.Text strong>密钥</Typography.Text>
                      <Typography.Text copyable>{setupResult.secret}</Typography.Text>
                      <Typography.Text strong>OTPAuth 地址</Typography.Text>
                      <Typography.Text copyable>{setupResult.otpauth_url}</Typography.Text>
                    </div>
                  </div>
                  <RecoveryCodes codes={recoveryCodes} />
                  <Divider />
                  <Form<MfaCodeFormValues>
                    form={enableForm}
                    layout="inline"
                    onFinish={(values) => enableMutation.mutate(values)}
                  >
                    <Form.Item
                      name="code"
                      rules={[{ required: true, message: "请输入多因素验证码" }]}
                    >
                      <Input inputMode="numeric" autoComplete="one-time-code" />
                    </Form.Item>
                    <Button
                      type="primary"
                      htmlType="submit"
                      loading={enableMutation.isPending}
                    >
                      启用
                    </Button>
                  </Form>
                </>
              ) : null}
            </Space>
          ) : (
            <div className="settings-grid-inner">
              <Form<MfaProtectedFormValues>
                form={disableForm}
                layout="vertical"
                onFinish={(values) => disableMutation.mutate(values)}
              >
                <Typography.Title level={4}>关闭多因素认证</Typography.Title>
                <Form.Item
                  name="password"
                  label="密码"
                  rules={[{ required: true, message: "请输入密码" }]}
                >
                  <Input.Password autoComplete="current-password" />
                </Form.Item>
                <Form.Item
                  name="code"
                  label="多因素验证码"
                  rules={[{ required: true, message: "请输入多因素验证码" }]}
                >
                  <Input autoComplete="one-time-code" />
                </Form.Item>
                <Button danger htmlType="submit" loading={disableMutation.isPending}>
                  关闭
                </Button>
              </Form>

              <Form<MfaProtectedFormValues>
                form={regenerateForm}
                layout="vertical"
                onFinish={(values) => regenerateMutation.mutate(values)}
              >
                <Typography.Title level={4}>恢复码</Typography.Title>
                <Form.Item
                  name="password"
                  label="密码"
                  rules={[{ required: true, message: "请输入密码" }]}
                >
                  <Input.Password autoComplete="current-password" />
                </Form.Item>
                <Form.Item
                  name="code"
                  label="多因素验证码"
                  rules={[{ required: true, message: "请输入多因素验证码" }]}
                >
                  <Input autoComplete="one-time-code" />
                </Form.Item>
                <Button
                  icon={<RefreshCw size={16} />}
                  htmlType="submit"
                  loading={regenerateMutation.isPending}
                >
                  重新生成
                </Button>
              </Form>
            </div>
          )}

          {user?.mfa_enabled && recoveryCodes.length ? (
            <>
              <Divider />
              <RecoveryCodes codes={recoveryCodes} />
            </>
          ) : null}
        </section>

        <section className="settings-panel settings-panel-wide">
          <div className="settings-panel-title settings-panel-title-split">
            <Space size={8}>
              <MonitorCheck size={18} />
              <Typography.Title level={3}>后台会话</Typography.Title>
            </Space>
            <Segmented<SessionStatusFilter>
              size="small"
              value={sessionStatusFilter}
              options={sessionStatusFilterOptions}
              onChange={setSessionStatusFilter}
            />
          </div>
          <Table<AdminSessionSummary>
            key={sessionStatusFilter}
            rowKey="id"
            size="small"
            columns={adminSessionColumns}
            dataSource={filteredAdminSessions}
            loading={adminSessionsQuery.isLoading}
            pagination={{
              pageSize: 10,
              showSizeChanger: false
            }}
            scroll={{ x: 900 }}
          />
        </section>

        {canReadSecurity ? (
          <section className="settings-panel settings-panel-wide">
            <div
              className="settings-panel-title"
              style={{ justifyContent: "space-between" }}
            >
              <Space size={8}>
                <KeyRound size={18} />
                <Typography.Title level={3}>JWT 密钥</Typography.Title>
              </Space>
              {canRotateSecurityKey ? (
                <ConfirmActionButton
                  title="轮换 JWT 密钥"
                  description="新的客户端 access token 会使用新密钥签发，旧密钥进入历史校验窗口。"
                  buttonProps={{
                    icon: <RefreshCw size={16} />
                  }}
                  loading={rotateJwtKeyMutation.isPending}
                  onConfirm={() => rotateJwtKeyMutation.mutate()}
                >
                  轮换
                </ConfirmActionButton>
              ) : null}
            </div>
            <Table<SigningKeySummary>
              rowKey="id"
              size="small"
              columns={jwtKeyColumns}
              dataSource={jwtKeysQuery.data?.items ?? []}
              loading={jwtKeysQuery.isLoading}
              pagination={false}
              scroll={{ x: 760 }}
            />
          </section>
        ) : null}
      </div>
    </section>
  );
}

function RecoveryCodes({ codes }: { codes: string[] }) {
  return (
    <div className="recovery-code-grid">
      {codes.map((code) => (
        <Typography.Text key={code} copyable>
          {code}
        </Typography.Text>
      ))}
    </div>
  );
}
