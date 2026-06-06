import {
  Alert,
  Button,
  Divider,
  Form,
  Input,
  Popconfirm,
  Space,
  Table,
  Tag,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { KeyRound, MailCheck, RefreshCw, ShieldCheck } from "lucide-react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { useState } from "react";

import {
  listGlobalJwtSigningKeys,
  rotateGlobalJwtSigningKey,
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
import { StatusTag } from "../../components/StatusTag";
import { useAuthStore } from "../../stores/authStore";
import { dateTime } from "../../utils/format";
import { hasPermission } from "../../utils/permissions";

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

export function SecurityPage() {
  const { user, tenant, roles, permissions, setProfile } = useAuthStore();
  const [passwordForm] = Form.useForm<PasswordFormValues>();
  const [enableForm] = Form.useForm<MfaCodeFormValues>();
  const [disableForm] = Form.useForm<MfaProtectedFormValues>();
  const [regenerateForm] = Form.useForm<MfaProtectedFormValues>();
  const [setupResult, setSetupResult] = useState<MfaSetupResult | null>(null);
  const [recoveryCodes, setRecoveryCodes] = useState<string[]>([]);
  const canReadSecurity = hasPermission(permissions, "security:read");
  const canRotateSecurityKey = hasPermission(permissions, "security:rotate_key");

  const jwtKeysQuery = useQuery({
    queryKey: ["admin", "global-jwt-signing-keys"],
    queryFn: listGlobalJwtSigningKeys,
    enabled: canReadSecurity
  });

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
      message.success(`password_changed:${data.revoked_sessions}`);
    }
  });

  const emailMutation = useMutation({
    mutationFn: requestEmailVerify,
    onSuccess: () => {
      message.success("email_verify_requested");
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
      message.success("mfa_enabled");
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
      message.success("mfa_disabled");
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
      message.success("recovery_codes_regenerated");
    }
  });

  const rotateJwtKeyMutation = useMutation({
    mutationFn: rotateGlobalJwtSigningKey,
    onSuccess: async (data) => {
      message.success(`jwt_key_rotated:${data.retired_key_count}`);
      await jwtKeysQuery.refetch();
    }
  });

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
            {user?.email_verified ? "email_verified" : "email_unverified"}
          </Tag>
          <Tag color={user?.mfa_enabled ? "green" : "default"}>
            {user?.mfa_enabled ? "mfa_enabled" : "mfa_disabled"}
          </Tag>
        </Space>
      </div>

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
            <Typography.Title level={3}>MFA</Typography.Title>
          </div>

          {!user?.mfa_enabled ? (
            <Space direction="vertical" size={14} className="settings-stack">
              <Button
                icon={<ShieldCheck size={16} />}
                onClick={() => setupMutation.mutate()}
                loading={setupMutation.isPending}
              >
                初始化 MFA
              </Button>

              {setupResult ? (
                <>
                  <Alert type="warning" message="recovery_codes_only_shown_once" showIcon />
                  <div className="secret-list">
                    <Typography.Text strong>Secret</Typography.Text>
                    <Typography.Text copyable>{setupResult.secret}</Typography.Text>
                    <Typography.Text strong>OTPAuth URL</Typography.Text>
                    <Typography.Text copyable>{setupResult.otpauth_url}</Typography.Text>
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
                      rules={[{ required: true, message: "请输入 MFA code" }]}
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
                <Typography.Title level={4}>关闭 MFA</Typography.Title>
                <Form.Item
                  name="password"
                  label="密码"
                  rules={[{ required: true, message: "请输入密码" }]}
                >
                  <Input.Password autoComplete="current-password" />
                </Form.Item>
                <Form.Item
                  name="code"
                  label="MFA code"
                  rules={[{ required: true, message: "请输入 MFA code" }]}
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
                  label="MFA code"
                  rules={[{ required: true, message: "请输入 MFA code" }]}
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

        {canReadSecurity ? (
          <section className="settings-panel settings-panel-wide">
            <div
              className="settings-panel-title"
              style={{ justifyContent: "space-between" }}
            >
              <Space size={8}>
                <KeyRound size={18} />
                <Typography.Title level={3}>JWT Key</Typography.Title>
              </Space>
              {canRotateSecurityKey ? (
                <Popconfirm
                  title="轮换 JWT key"
                  onConfirm={() => rotateJwtKeyMutation.mutate()}
                >
                  <Button
                    icon={<RefreshCw size={16} />}
                    loading={rotateJwtKeyMutation.isPending}
                  >
                    轮换
                  </Button>
                </Popconfirm>
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
