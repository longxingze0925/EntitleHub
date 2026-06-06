import { Alert, Button, Form, Input, Typography, message } from "antd";
import { CheckCircle2, KeyRound } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { Link, useSearchParams } from "react-router-dom";

import { confirmEmailVerify } from "../../api/auth";
import { useAuthStore } from "../../stores/authStore";
import { tApiError, tMessage } from "../../utils/i18n";

interface EmailVerifyFormValues {
  token: string;
}

export function EmailVerifyPage() {
  const [searchParams] = useSearchParams();
  const token = searchParams.get("token") ?? "";
  const { user, tenant, roles, permissions, setProfile } = useAuthStore();

  const mutation = useMutation({
    mutationFn: (values: EmailVerifyFormValues) =>
      confirmEmailVerify(values.token.trim()),
    onSuccess: (data) => {
      if (user && tenant && user.id === data.team_member_id) {
        setProfile({
          user: { ...user, email_verified: data.email_verified },
          tenant,
          roles,
          permissions
        });
      }
      message.success(tMessage("email_verified"));
    }
  });

  const error = tApiError(mutation.error);

  return (
    <main className="login-screen">
      <section className="login-panel">
        <div className="login-brand">
          <Typography.Title level={1}>邮箱验证</Typography.Title>
        </div>

        <Form<EmailVerifyFormValues>
          layout="vertical"
          onFinish={(values) => mutation.mutate(values)}
          initialValues={{ token }}
        >
          <Form.Item
            name="token"
            label="令牌"
            rules={[{ required: true, message: "请输入令牌" }]}
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

        <div className="login-actions">
          <Link to="/login">返回登录</Link>
        </div>
      </section>
    </main>
  );
}
