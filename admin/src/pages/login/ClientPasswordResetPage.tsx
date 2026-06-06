import { Alert, Button, Form, Input, Typography, message } from "antd";
import { KeyRound, Save } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { useSearchParams } from "react-router-dom";

import { confirmClientPasswordReset } from "../../api/auth";
import { ApiError } from "../../api/client";

interface ClientPasswordResetFormValues {
  token: string;
  new_password: string;
}

export function ClientPasswordResetPage() {
  const [searchParams] = useSearchParams();
  const token = searchParams.get("token") ?? "";

  const mutation = useMutation({
    mutationFn: (values: ClientPasswordResetFormValues) =>
      confirmClientPasswordReset({
        token: values.token.trim(),
        new_password: values.new_password
      }),
    onSuccess: () => {
      message.success("password_reset_confirmed");
    }
  });

  const error =
    mutation.error instanceof ApiError
      ? mutation.error.message
      : mutation.error
        ? "service_unavailable"
        : null;

  return (
    <main className="login-screen">
      <section className="login-panel">
        <div className="login-brand">
          <Typography.Title level={1}>重置密码</Typography.Title>
        </div>

        <Form<ClientPasswordResetFormValues>
          layout="vertical"
          onFinish={(values) => mutation.mutate(values)}
          initialValues={{ token }}
        >
          <Form.Item
            name="token"
            label="Token"
            rules={[{ required: true, message: "请输入 token" }]}
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

          {error ? (
            <Alert type="error" message={error} showIcon className="login-alert" />
          ) : null}

          <Button
            type="primary"
            htmlType="submit"
            icon={<Save size={16} />}
            loading={mutation.isPending}
            block
          >
            保存
          </Button>
        </Form>
      </section>
    </main>
  );
}
