import { Alert, Button, Form, Input, Typography } from "antd";
import { LockKeyhole, LogIn, Mail } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { Link, Navigate, useLocation, useNavigate } from "react-router-dom";

import { login } from "../../api/auth";
import { useAuthStore } from "../../stores/authStore";
import { tApiError } from "../../utils/i18n";

interface LoginFormValues {
  email: string;
  password: string;
  mfa_code?: string;
}

export function LoginPage() {
  const navigate = useNavigate();
  const location = useLocation();
  const { user, setProfile } = useAuthStore();
  const from = (location.state as { from?: string } | null)?.from ?? "/";

  const mutation = useMutation({
    mutationFn: (values: LoginFormValues) =>
      login({
        email: values.email.trim(),
        password: values.password,
        mfa_code: values.mfa_code?.trim() || undefined
      }),
    onSuccess: (data) => {
      setProfile(data);
      navigate(from, { replace: true });
    }
  });

  if (user) {
    return <Navigate to="/" replace />;
  }

  const error = tApiError(mutation.error);

  return (
    <main className="login-screen">
      <section className="login-panel">
        <div className="login-brand">
          <Typography.Title level={1}>EntitleHub</Typography.Title>
          <Typography.Text type="secondary">
            安全管理团队、客户、授权、设备和分发内容
          </Typography.Text>
        </div>

        <Form<LoginFormValues>
          layout="vertical"
          onFinish={(values) => mutation.mutate(values)}
          initialValues={{ email: "", password: "" }}
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

          <Form.Item
            name="password"
            label="密码"
            rules={[{ required: true, message: "请输入密码" }]}
          >
            <Input.Password
              prefix={<LockKeyhole size={16} />}
              autoComplete="current-password"
              placeholder="请输入密码"
            />
          </Form.Item>

          <Form.Item name="mfa_code" label="多因素验证码">
            <Input inputMode="numeric" autoComplete="one-time-code" />
          </Form.Item>

          {error ? (
            <Alert
              type="error"
              message={error}
              showIcon
              className="login-alert"
            />
          ) : null}

          <Button
            type="primary"
            htmlType="submit"
            icon={<LogIn size={16} />}
            loading={mutation.isPending}
            block
          >
            登录
          </Button>
          <div className="login-actions">
            <Link to="/admin/password-reset">忘记密码</Link>
          </div>
        </Form>
      </section>
    </main>
  );
}
