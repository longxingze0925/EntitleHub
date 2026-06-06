import { Alert, Button, Form, Input, Typography, message } from "antd";
import { KeyRound, Mail, Save } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { Link, useNavigate, useSearchParams } from "react-router-dom";

import {
  confirmPasswordReset,
  requestPasswordReset
} from "../../api/auth";
import { tApiError, tMessage } from "../../utils/i18n";

interface RequestResetFormValues {
  email: string;
}

interface ConfirmResetFormValues {
  token: string;
  new_password: string;
}

export function PasswordResetPage() {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const token = searchParams.get("token") ?? "";
  const [requestForm] = Form.useForm<RequestResetFormValues>();
  const [confirmForm] = Form.useForm<ConfirmResetFormValues>();

  const requestMutation = useMutation({
    mutationFn: (values: RequestResetFormValues) =>
      requestPasswordReset(values.email.trim()),
    onSuccess: () => {
      requestForm.resetFields();
      message.success(tMessage("password_reset_requested"));
    }
  });

  const confirmMutation = useMutation({
    mutationFn: (values: ConfirmResetFormValues) =>
      confirmPasswordReset({
        token: values.token.trim(),
        new_password: values.new_password
      }),
    onSuccess: () => {
      message.success(tMessage("password_reset_confirmed"));
      navigate("/login", { replace: true });
    }
  });

  const requestError = errorMessage(requestMutation.error);
  const confirmError = errorMessage(confirmMutation.error);

  return (
    <main className="login-screen">
      <section className="login-panel">
        <div className="login-brand">
          <Typography.Title level={1}>重置密码</Typography.Title>
        </div>

        {token ? (
          <Form<ConfirmResetFormValues>
            form={confirmForm}
            layout="vertical"
            onFinish={(values) => confirmMutation.mutate(values)}
            initialValues={{ token }}
          >
            <Form.Item
              name="token"
              label="令牌"
              rules={[{ required: true, message: "请输入令牌" }]}
            >
              <Input prefix={<KeyRound size={16} />} />
            </Form.Item>
            <Form.Item
              name="new_password"
              label="新密码"
              rules={[{ required: true, message: "请输入新密码" }]}
            >
              <Input.Password autoComplete="new-password" />
            </Form.Item>
            {confirmError ? (
              <Alert
                type="error"
                message={confirmError}
                showIcon
                className="login-alert"
              />
            ) : null}
            <Button
              type="primary"
              htmlType="submit"
              icon={<Save size={16} />}
              loading={confirmMutation.isPending}
              block
            >
              保存
            </Button>
          </Form>
        ) : (
          <Form<RequestResetFormValues>
            form={requestForm}
            layout="vertical"
            onFinish={(values) => requestMutation.mutate(values)}
          >
            <Form.Item
              name="email"
              label="邮箱"
              rules={[
                { required: true, message: "请输入邮箱" },
                { type: "email", message: "邮箱格式不正确" }
              ]}
            >
              <Input
                prefix={<Mail size={16} />}
                autoComplete="email"
                placeholder="admin@example.com"
              />
            </Form.Item>
            {requestError ? (
              <Alert
                type="error"
                message={requestError}
                showIcon
                className="login-alert"
              />
            ) : null}
            <Button
              type="primary"
              htmlType="submit"
              icon={<Mail size={16} />}
              loading={requestMutation.isPending}
              block
            >
              发送
            </Button>
          </Form>
        )}

        <div className="login-actions">
          <Link to="/login">返回登录</Link>
        </div>
      </section>
    </main>
  );
}

function errorMessage(error: unknown): string | null {
  return tApiError(error);
}
