import { Alert, Button, Form, Input, Typography, message } from "antd";
import { CheckCircle2, KeyRound, LockKeyhole, UserRound } from "lucide-react";
import { useMutation } from "@tanstack/react-query";
import { Link, useNavigate, useSearchParams } from "react-router-dom";

import { acceptTeamInvitation } from "../../api/auth";
import { tApiError, tMessage } from "../../utils/i18n";

interface TeamInvitationFormValues {
  token: string;
  name: string;
  password: string;
  confirm_password: string;
}

export function TeamInvitationAcceptPage() {
  const navigate = useNavigate();
  const [form] = Form.useForm<TeamInvitationFormValues>();
  const [searchParams] = useSearchParams();
  const token = searchParams.get("token") ?? "";

  const mutation = useMutation({
    mutationFn: (values: TeamInvitationFormValues) =>
      acceptTeamInvitation({
        token: values.token.trim(),
        name: values.name.trim(),
        password: values.password
      }),
    onSuccess: () => {
      message.success(tMessage("team_invitation_accepted"));
      navigate("/login", { replace: true });
    }
  });

  const error = tApiError(mutation.error);

  return (
    <main className="login-screen">
      <section className="login-panel">
        <div className="login-brand">
          <Typography.Title level={1}>接受团队邀请</Typography.Title>
          <Typography.Text type="secondary">
            设置姓名和登录密码后即可进入后台
          </Typography.Text>
        </div>

        <Form<TeamInvitationFormValues>
          form={form}
          layout="vertical"
          onFinish={(values) => mutation.mutate(values)}
          initialValues={{ token }}
        >
          <Form.Item
            name="token"
            label="邀请令牌"
            rules={[{ required: true, message: "请输入邀请令牌" }]}
          >
            <Input prefix={<KeyRound size={16} />} />
          </Form.Item>

          <Form.Item
            name="name"
            label="姓名"
            rules={[{ required: true, message: "请输入姓名" }]}
          >
            <Input
              prefix={<UserRound size={16} />}
              autoComplete="name"
              placeholder="请输入姓名"
            />
          </Form.Item>

          <Form.Item
            name="password"
            label="密码"
            rules={[{ required: true, message: "请输入密码" }]}
          >
            <Input.Password
              prefix={<LockKeyhole size={16} />}
              autoComplete="new-password"
              placeholder="请输入密码"
            />
          </Form.Item>

          <Form.Item
            name="confirm_password"
            label="确认密码"
            dependencies={["password"]}
            rules={[
              { required: true, message: "请再次输入密码" },
              ({ getFieldValue }) => ({
                validator(_, value) {
                  if (!value || getFieldValue("password") === value) {
                    return Promise.resolve();
                  }

                  return Promise.reject(new Error("两次输入的密码不一致"));
                }
              })
            ]}
          >
            <Input.Password
              prefix={<LockKeyhole size={16} />}
              autoComplete="new-password"
              placeholder="请再次输入密码"
            />
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
            icon={<CheckCircle2 size={16} />}
            loading={mutation.isPending}
            block
          >
            激活账号
          </Button>
        </Form>

        <div className="login-actions">
          <Link to="/login">返回登录</Link>
        </div>
      </section>
    </main>
  );
}
