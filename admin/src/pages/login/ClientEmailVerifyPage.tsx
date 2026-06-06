import { Alert, Button, Form, Input, Typography, message } from "antd";
import { CheckCircle2, KeyRound } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { useSearchParams } from "react-router-dom";

import { confirmClientEmailVerify } from "../../api/auth";
import { ApiError } from "../../api/client";

interface ClientEmailVerifyFormValues {
  token: string;
}

export function ClientEmailVerifyPage() {
  const [searchParams] = useSearchParams();
  const token = searchParams.get("token") ?? "";

  const mutation = useMutation({
    mutationFn: (values: ClientEmailVerifyFormValues) =>
      confirmClientEmailVerify(values.token.trim()),
    onSuccess: () => {
      message.success("email_verified");
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
          <Typography.Title level={1}>邮箱验证</Typography.Title>
        </div>

        <Form<ClientEmailVerifyFormValues>
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

          {error ? (
            <Alert type="error" message={error} showIcon className="login-alert" />
          ) : null}

          <Button
            type="primary"
            htmlType="submit"
            icon={<CheckCircle2 size={16} />}
            loading={mutation.isPending}
            block
          >
            验证
          </Button>
        </Form>
      </section>
    </main>
  );
}
