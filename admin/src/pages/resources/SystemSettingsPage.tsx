import {
  Alert,
  Button,
  Checkbox,
  Form,
  Input,
  InputNumber,
  Modal,
  Space,
  Switch,
  Table,
  Tag,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import type { FormInstance } from "antd/es/form";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Mail, Pencil, Plus, RefreshCw, Send } from "lucide-react";
import { useEffect, useState } from "react";

import {
  getEmailSettings,
  listSystemSettings,
  testEmailSettings,
  updateEmailSettings,
  updateSystemSetting,
  type EmailSettings,
  type SystemSetting
} from "../../api/admin";
import { useAuthStore } from "../../stores/authStore";
import { dateTime } from "../../utils/format";
import { tMessage } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

interface SettingFormValues {
  key: string;
  value: string;
}

interface EmailSettingsFormValues {
  enabled: boolean;
  smtp_host: string;
  smtp_port: number;
  smtp_user?: string;
  smtp_password?: string;
  clear_password?: boolean;
  smtp_from: string;
  test_to?: string;
}

const defaultValue = `{
  "enabled": true
}`;

export function SystemSettingsPage() {
  const [form] = Form.useForm<SettingFormValues>();
  const [emailForm] = Form.useForm<EmailSettingsFormValues>();
  const [editing, setEditing] = useState<SystemSetting | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const queryClient = useQueryClient();
  const permissions = useAuthStore((state) => state.permissions);
  const canUpdate = hasPermission(permissions, "system:update");

  const query = useQuery({
    queryKey: ["admin", "system-settings"],
    queryFn: listSystemSettings
  });
  const emailQuery = useQuery({
    queryKey: ["admin", "system", "email-settings"],
    queryFn: getEmailSettings
  });

  const mutation = useMutation({
    mutationFn: (values: SettingFormValues) =>
      updateSystemSetting({
        key: values.key.trim(),
        payload: { value: JSON.parse(values.value) }
      }),
    onSuccess: () => {
      message.success(tMessage("system_setting_saved"));
      setModalOpen(false);
      setEditing(null);
      form.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin", "system-settings"] });
    }
  });
  const emailMutation = useMutation({
    mutationFn: (values: EmailSettingsFormValues) => {
      const smtpUser = cleanOptional(values.smtp_user);
      return updateEmailSettings({
        enabled: values.enabled,
        smtp_host: values.smtp_host.trim(),
        smtp_port: values.smtp_port,
        smtp_user: smtpUser,
        smtp_from: values.smtp_from.trim(),
        smtp_password: values.clear_password ? undefined : cleanOptional(values.smtp_password),
        clear_password: Boolean(values.clear_password || !smtpUser)
      });
    },
    onSuccess: (settings) => {
      message.success("邮件服务配置已保存");
      applyEmailSettings(emailForm, settings, emailForm.getFieldValue("test_to"));
      queryClient.invalidateQueries({
        queryKey: ["admin", "system", "email-settings"]
      });
    }
  });
  const emailTestMutation = useMutation({
    mutationFn: (to: string) =>
      testEmailSettings({
        to,
        confirm_delivery: true
      }),
    onSuccess: (settings) => {
      message.success("测试邮件已发送");
      applyEmailSettings(emailForm, settings, emailForm.getFieldValue("test_to"));
      queryClient.invalidateQueries({
        queryKey: ["admin", "system", "email-settings"]
      });
    }
  });

  useEffect(() => {
    if (emailQuery.data) {
      applyEmailSettings(emailForm, emailQuery.data);
    }
  }, [emailForm, emailQuery.data]);

  const openCreate = () => {
    setEditing(null);
    form.setFieldsValue({ key: "", value: defaultValue });
    setModalOpen(true);
  };

  const openEdit = (setting: SystemSetting) => {
    setEditing(setting);
    form.setFieldsValue({
      key: setting.key,
      value: stringifyJson(setting.value)
    });
    setModalOpen(true);
  };

  const columns: ColumnsType<SystemSetting> = [
    {
      title: "配置键",
      dataIndex: "key",
      key: "key",
      width: 240,
      render: (value: string) => <Typography.Text copyable>{value}</Typography.Text>
    },
    {
      title: "配置值",
      dataIndex: "value",
      key: "value",
      render: (value: unknown) => (
        <pre className="json-view setting-json-view">{stringifyJson(value)}</pre>
      )
    },
    {
      title: "更新时间",
      dataIndex: "updated_at",
      key: "updated_at",
      width: 190,
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 90,
      render: (_, record) => (
        <Button
          size="small"
          icon={<Pencil size={14} />}
          disabled={!canUpdate}
          onClick={() => openEdit(record)}
        />
      )
    }
  ];
  const confirmEmailTest = () => {
    const to = emailForm.getFieldValue("test_to")?.trim();
    if (!to) {
      message.error("请输入测试收件人");
      return;
    }

    Modal.confirm({
      title: "发送测试邮件",
      content: `会向 ${to} 发送一封真实测试邮件，请先确认 SMTP 配置已保存。`,
      okText: "发送",
      cancelText: "取消",
      onOk: () => emailTestMutation.mutateAsync(to)
    });
  };

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>系统配置</Typography.Title>
          <Typography.Text type="secondary">全局运行配置</Typography.Text>
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

      {query.error ? (
        <Alert type="error" message={tMessage("system_settings_load_failed")} />
      ) : null}
      {mutation.error ? (
        <Alert type="error" message={tMessage("system_setting_save_failed")} />
      ) : null}
      {emailQuery.error ? <Alert type="error" message="邮件服务配置加载失败" /> : null}
      {emailMutation.error ? <Alert type="error" message="邮件服务配置保存失败" /> : null}
      {emailTestMutation.error ? <Alert type="error" message="测试邮件发送失败" /> : null}

      <div className="settings-panel settings-panel-wide">
        <div className="settings-panel-title settings-panel-title-split">
          <Space>
            <Mail size={18} />
            <Typography.Title level={3}>邮件服务</Typography.Title>
            {emailQuery.data ? <EmailSourceTag settings={emailQuery.data} /> : null}
          </Space>
          <Space>
            <Button
              icon={<RefreshCw size={16} />}
              onClick={() => emailQuery.refetch()}
              loading={emailQuery.isFetching}
            />
            <Button
              icon={<Send size={16} />}
              disabled={!canUpdate || !emailQuery.data?.enabled}
              loading={emailTestMutation.isPending}
              onClick={confirmEmailTest}
            >
              测试
            </Button>
            <Button
              type="primary"
              disabled={!canUpdate}
              loading={emailMutation.isPending}
              onClick={() => emailForm.submit()}
            >
              保存
            </Button>
          </Space>
        </div>
        <Typography.Text type="secondary">
          用于忘记密码、邮箱验证、客户密码重置等系统邮件。后台保存后会动态生效，不需要重启服务。
        </Typography.Text>
        <Form<EmailSettingsFormValues>
          form={emailForm}
          layout="vertical"
          className="settings-stack"
          onFinish={(values) => emailMutation.mutate(values)}
        >
          <div className="settings-grid-inner">
            <Form.Item name="enabled" label="启用" valuePropName="checked">
              <Switch />
            </Form.Item>
            <Form.Item
              name="smtp_host"
              label="SMTP 主机"
              rules={[
                ({ getFieldValue }) => ({
                  validator: (_, value) => {
                    if (getFieldValue("enabled") && !value?.trim()) {
                      return Promise.reject(new Error("启用邮件服务时必须填写 SMTP 主机"));
                    }
                    return Promise.resolve();
                  }
                })
              ]}
            >
              <Input placeholder="smtp.example.com" />
            </Form.Item>
            <Form.Item
              name="smtp_port"
              label="SMTP 端口"
              rules={[{ required: true, message: "请输入 SMTP 端口" }]}
            >
              <InputNumber min={1} max={65535} className="form-number" />
            </Form.Item>
            <Form.Item name="smtp_user" label="SMTP 用户名">
              <Input placeholder="通常是发件邮箱" />
            </Form.Item>
            <Form.Item noStyle shouldUpdate>
              {({ getFieldValue }) => (
                <Form.Item
                  name="smtp_password"
                  label="SMTP 授权码 / 密码"
                  rules={[
                    {
                      validator: (_, value) => {
                        const enabled = getFieldValue("enabled");
                        const smtpUser = cleanOptional(getFieldValue("smtp_user"));
                        const clearPassword = Boolean(getFieldValue("clear_password"));
                        const hasSavedPassword = Boolean(emailQuery.data?.smtp_password_configured);
                        if (
                          enabled &&
                          smtpUser &&
                          !clearPassword &&
                          !hasSavedPassword &&
                          !value?.trim()
                        ) {
                          return Promise.reject(
                            new Error("启用认证 SMTP 时必须填写授权码")
                          );
                        }
                        if (enabled && smtpUser && clearPassword) {
                          return Promise.reject(
                            new Error("启用认证 SMTP 时不能清除授权码")
                          );
                        }
                        return Promise.resolve();
                      }
                    }
                  ]}
                >
                  <Input.Password
                    disabled={Boolean(getFieldValue("clear_password"))}
                    placeholder={
                      emailQuery.data?.smtp_password_configured
                        ? "已配置，留空则不修改"
                        : "请输入邮箱授权码"
                    }
                  />
                </Form.Item>
              )}
            </Form.Item>
            {emailQuery.data?.smtp_password_configured ? (
              <Form.Item name="clear_password" label="清除授权码" valuePropName="checked">
                <Checkbox>保存时删除已配置的 SMTP 授权码</Checkbox>
              </Form.Item>
            ) : null}
            <Form.Item
              name="smtp_from"
              label="发件邮箱"
              rules={[
                ({ getFieldValue }) => ({
                  validator: (_, value) => {
                    if (getFieldValue("enabled") && !value?.trim()) {
                      return Promise.reject(new Error("启用邮件服务时必须填写发件邮箱"));
                    }
                    if (value?.trim() && !isEmail(value)) {
                      return Promise.reject(new Error("邮箱格式不正确"));
                    }
                    return Promise.resolve();
                  }
                })
              ]}
            >
              <Input placeholder="noreply@example.com" />
            </Form.Item>
            <Form.Item
              name="test_to"
              label="测试收件人"
              rules={[{ type: "email", message: "邮箱格式不正确" }]}
            >
              <Input placeholder="用于发送测试邮件" />
            </Form.Item>
          </div>
        </Form>
        {emailQuery.data?.last_test_status ? (
          <Typography.Text type="secondary">
            最近测试：{emailQuery.data.last_test_status === "success" ? "成功" : "失败"}
            {emailQuery.data.last_test_at ? ` · ${dateTime(emailQuery.data.last_test_at)}` : ""}
            {emailQuery.data.last_test_error ? ` · ${emailQuery.data.last_test_error}` : ""}
          </Typography.Text>
        ) : null}
      </div>

      <Table
        rowKey="key"
        loading={query.isLoading}
        columns={columns}
        dataSource={query.data?.items ?? []}
        pagination={false}
        locale={{ emptyText: "暂无数据" }}
      />

      <Modal
        title={editing ? "编辑配置" : "新增配置"}
        open={modalOpen}
        onCancel={() => {
          setModalOpen(false);
          setEditing(null);
        }}
        onOk={() => form.submit()}
        confirmLoading={mutation.isPending}
        width={760}
        destroyOnClose
      >
        <Form<SettingFormValues>
          form={form}
          layout="vertical"
          onFinish={(values) => mutation.mutate(values)}
        >
          <Form.Item
            name="key"
            label="配置键"
            rules={[
              { required: true, message: "请输入配置键" },
              {
                pattern: /^[A-Za-z0-9_.:-]+$/,
                message: "配置键只能包含字母、数字、_、-、.、:"
              }
            ]}
          >
            <Input disabled={Boolean(editing)} />
          </Form.Item>
          <Form.Item
            name="value"
            label="配置值"
            rules={[
              { required: true, message: "请输入 JSON" },
              {
                validator: (_, value) => {
                  try {
                    JSON.parse(value);
                    return Promise.resolve();
                  } catch {
                    return Promise.reject(new Error("JSON 格式不正确"));
                  }
                }
              }
            ]}
          >
            <Input.TextArea className="settings-json-editor" rows={12} />
          </Form.Item>
        </Form>
      </Modal>
    </section>
  );
}

function stringifyJson(value: unknown): string {
  return JSON.stringify(value, null, 2);
}

function cleanOptional(value?: string): string | undefined {
  const trimmed = value?.trim();
  return trimmed ? trimmed : undefined;
}

function isEmail(value: string): boolean {
  return /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(value.trim());
}

function applyEmailSettings(
  form: FormInstance<EmailSettingsFormValues>,
  settings: EmailSettings,
  testTo?: string
) {
  form.setFieldsValue({
    enabled: settings.enabled,
    smtp_host: settings.smtp_host,
    smtp_port: settings.smtp_port,
    smtp_user: settings.smtp_user ?? undefined,
    smtp_password: "",
    clear_password: false,
    smtp_from: settings.smtp_from,
    test_to: testTo?.trim() || settings.smtp_from || undefined
  });
}

function EmailSourceTag({ settings }: { settings: EmailSettings }) {
  const sourceText = settings.source === "database" ? "后台配置" : "服务器配置";
  const color = settings.enabled ? "green" : "default";

  return (
    <Space size={6}>
      <Tag color={color}>{settings.enabled ? "启用" : "停用"}</Tag>
      <Tag>{sourceText}</Tag>
      <Tag color={settings.smtp_password_configured ? "blue" : "red"}>
        {settings.smtp_password_configured ? "授权码已配置" : "缺少授权码"}
      </Tag>
    </Space>
  );
}
