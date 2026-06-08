import {
  Alert,
  Button,
  Form,
  Input,
  Modal,
  Select,
  Space,
  Table,
  Tag,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Ban, Edit3, Plus, RefreshCw } from "lucide-react";
import { useState } from "react";

import {
  disableTeamMember,
  inviteTeamMember,
  listRoles,
  listTeamMembers,
  updateTeamMemberRoles,
  type InvitationResult,
  type InviteTeamMemberPayload,
  type TeamMember,
  type UpdateTeamMemberRolesPayload
} from "../../api/admin";
import { ConfirmActionButton } from "../../components/ConfirmActionButton";
import { HistoryToggle } from "../../components/HistoryToggle";
import { StatusTag } from "../../components/StatusTag";
import { useAuthStore } from "../../stores/authStore";
import { dateTime } from "../../utils/format";
import { tMessage, tRoleName } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

interface InvitationView extends InvitationResult {
  email: string;
}

export function TeamPage() {
  const [inviteOpen, setInviteOpen] = useState(false);
  const [includeHistory, setIncludeHistory] = useState(false);
  const [editingMember, setEditingMember] = useState<TeamMember | null>(null);
  const [invitation, setInvitation] = useState<InvitationView | null>(null);
  const [inviteForm] = Form.useForm<InviteTeamMemberPayload>();
  const [roleForm] = Form.useForm<UpdateTeamMemberRolesPayload>();
  const permissions = useAuthStore((state) => state.permissions);
  const canReadRoles = hasPermission(permissions, "role:read");
  const canInvite = hasPermission(permissions, "member:invite") && canReadRoles;
  const canUpdate = hasPermission(permissions, "member:update") && canReadRoles;
  const canDisable = hasPermission(permissions, "member:disable");
  const query = useQuery({
    queryKey: ["admin", "team", includeHistory],
    queryFn: () => listTeamMembers({ include_history: includeHistory })
  });
  const rolesQuery = useQuery({
    queryKey: ["admin", "roles"],
    queryFn: listRoles,
    enabled: canInvite || canUpdate
  });
  const inviteMutation = useMutation({
    mutationFn: inviteTeamMember,
    onSuccess: async (data) => {
      message.success(tMessage("member_invited"));
      setInvitation({
        ...data.invitation,
        email: data.member.email
      });
      setInviteOpen(false);
      inviteForm.resetFields();
      await query.refetch();
    }
  });
  const roleMutation = useMutation({
    mutationFn: updateTeamMemberRoles,
    onSuccess: async () => {
      message.success(tMessage("member_roles_updated"));
      setEditingMember(null);
      roleForm.resetFields();
      await query.refetch();
    }
  });
  const disableMutation = useMutation({
    mutationFn: disableTeamMember,
    onSuccess: async (data) => {
      message.success(tMessage(`member_disabled:${data.revoked_sessions ?? 0}`));
      await query.refetch();
    }
  });

  const openRoles = (member: TeamMember) => {
    setEditingMember(member);
    roleForm.setFieldsValue({
      role_codes: member.roles.map((role) => role.code)
    });
  };

  const columns: ColumnsType<TeamMember> = [
    {
      title: "成员",
      dataIndex: "name",
      key: "name",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text strong>{record.name}</Typography.Text>
          <Typography.Text type="secondary">{record.email}</Typography.Text>
        </Space>
      )
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 110,
      render: (value) => <StatusTag value={value} />
    },
    {
      title: "角色",
      dataIndex: "roles",
      key: "roles",
      render: (roles: TeamMember["roles"]) =>
        roles.length > 0
          ? roles.map((role) => (
              <Tag key={role.code}>{tRoleName(role.code, role.name)}</Tag>
            ))
          : "-"
    },
    {
      title: "多因素认证",
      dataIndex: "mfa_enabled",
      key: "mfa_enabled",
      width: 90,
      render: (enabled: boolean) =>
        enabled ? <Tag color="green">已开启</Tag> : <Tag>未开启</Tag>
    },
    {
      title: "邮箱验证",
      dataIndex: "email_verified",
      key: "email_verified",
      width: 110,
      render: (verified: boolean) =>
        verified ? <Tag color="green">已验证</Tag> : <Tag>待验证</Tag>
    },
    {
      title: "操作",
      key: "actions",
      width: 180,
      render: (_, record) => (
        <Space>
          {canUpdate ? (
            <Button
              size="small"
              icon={<Edit3 size={14} />}
              onClick={() => openRoles(record)}
            >
              角色
            </Button>
          ) : null}
          {canDisable && record.status !== "disabled" ? (
            <ConfirmActionButton
              title="禁用成员"
              description="禁用后会撤销该成员的后台登录会话。"
              buttonProps={{
                size: "small",
                icon: <Ban size={14} />
              }}
              loading={disableMutation.isPending}
              onConfirm={() => disableMutation.mutate(record.id)}
            >
              禁用
            </ConfirmActionButton>
          ) : null}
        </Space>
      )
    }
  ];
  const roleOptions =
    rolesQuery.data?.items.map((role) => ({
      value: role.code,
      label: tRoleName(role.code, role.name)
    })) ?? [];

  const submitInvite = (values: InviteTeamMemberPayload) => {
    inviteMutation.mutate({
      email: values.email.trim(),
      role_codes: values.role_codes
    });
  };

  const submitRoles = (values: UpdateTeamMemberRolesPayload) => {
    if (!editingMember) {
      return;
    }

    roleMutation.mutate({
      id: editingMember.id,
      payload: {
        role_codes: values.role_codes
      }
    });
  };

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>团队成员</Typography.Title>
          <Typography.Text type="secondary">管理员账号、角色和成员状态</Typography.Text>
        </div>
        <Space>
          <HistoryToggle
            checked={includeHistory}
            onChange={setIncludeHistory}
          />
          <Button icon={<RefreshCw size={16} />} onClick={() => query.refetch()} />
          {canInvite ? (
            <Button
              type="primary"
              icon={<Plus size={16} />}
              onClick={() => setInviteOpen(true)}
            >
              邀请成员
            </Button>
          ) : null}
        </Space>
      </div>
      {query.error ? (
        <Alert type="error" message={tMessage("team_members_load_failed")} />
      ) : null}
      {rolesQuery.error ? (
        <Alert type="error" message={tMessage("roles_load_failed")} />
      ) : null}
      {inviteMutation.error ? (
        <Alert type="error" message={tMessage("team_member_invite_failed")} />
      ) : null}
      {roleMutation.error ? (
        <Alert type="error" message={tMessage("team_member_roles_update_failed")} />
      ) : null}
      {disableMutation.error ? (
        <Alert type="error" message={tMessage("team_member_disable_failed")} />
      ) : null}
      <Table
        rowKey="id"
        loading={query.isLoading}
        columns={columns}
        dataSource={query.data?.items ?? []}
        pagination={false}
        locale={{ emptyText: "暂无数据" }}
      />

      <Modal
        title="邀请成员"
        open={inviteOpen}
        onCancel={() => setInviteOpen(false)}
        onOk={() => inviteForm.submit()}
        confirmLoading={inviteMutation.isPending}
        destroyOnClose
      >
        <Form<InviteTeamMemberPayload>
          form={inviteForm}
          layout="vertical"
          onFinish={submitInvite}
          initialValues={{ role_codes: ["viewer"] }}
        >
          <Form.Item
            name="email"
            label="邮箱"
            rules={[
              { required: true, message: "请输入邮箱" },
              { type: "email", message: "邮箱格式不正确" }
            ]}
          >
            <Input autoComplete="email" />
          </Form.Item>
          <Form.Item
            name="role_codes"
            label="角色"
            rules={[{ required: true, message: "请选择角色" }]}
          >
            <Select
              mode="multiple"
              optionFilterProp="label"
              labelRender={(item) => tRoleName(String(item.value ?? ""))}
              options={roleOptions}
              loading={rolesQuery.isLoading}
            />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="编辑角色"
        open={Boolean(editingMember)}
        onCancel={() => {
          setEditingMember(null);
          roleForm.resetFields();
        }}
        onOk={() => roleForm.submit()}
        confirmLoading={roleMutation.isPending}
        destroyOnClose
      >
        <Form<UpdateTeamMemberRolesPayload>
          form={roleForm}
          layout="vertical"
          onFinish={submitRoles}
        >
          <Form.Item label="成员">
            <Typography.Text>{editingMember?.email}</Typography.Text>
          </Form.Item>
          <Form.Item
            name="role_codes"
            label="角色"
            rules={[{ required: true, message: "请选择角色" }]}
          >
            <Select
              mode="multiple"
              optionFilterProp="label"
              labelRender={(item) => tRoleName(String(item.value ?? ""))}
              options={roleOptions}
              loading={rolesQuery.isLoading}
            />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="邀请链接"
        open={Boolean(invitation)}
        onCancel={() => setInvitation(null)}
        onOk={() => setInvitation(null)}
      >
        <Space direction="vertical" size={12} className="token-result">
          <Alert
            type="info"
            showIcon
            message="邀请邮件会进入发送队列；如果邮件未送达，可复制链接手动发送给成员。"
          />
          <Typography.Text type="secondary">成员</Typography.Text>
          <Typography.Text copyable>{invitation?.email}</Typography.Text>
          <Typography.Text type="secondary">邀请链接</Typography.Text>
          <Typography.Paragraph
            copyable={{ text: invitation ? invitationLink(invitation.token) : "" }}
          >
            {invitation ? invitationLink(invitation.token) : "-"}
          </Typography.Paragraph>
          <Typography.Text type="secondary">邀请令牌</Typography.Text>
          <Typography.Text copyable>{invitation?.token}</Typography.Text>
          <Typography.Text type="secondary">
            有效期至 {dateTime(invitation?.expires_at)}
          </Typography.Text>
        </Space>
      </Modal>
    </section>
  );
}

function invitationLink(token: string): string {
  const baseUrl = window.location.origin;
  const encodedToken = encodeURIComponent(token);

  return `${baseUrl}/team/invitations/accept?token=${encodedToken}`;
}
