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
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Edit3, Plus, RefreshCw, Trash2 } from "lucide-react";
import { useMemo, useState } from "react";

import {
  createRole,
  deleteRole,
  listPermissions,
  listRoles,
  updateRole,
  type CreateRolePayload,
  type PermissionSummary,
  type RoleDetail,
  type UpdateRolePayload
} from "../../api/admin";
import { ConfirmActionButton } from "../../components/ConfirmActionButton";
import { useAuthStore } from "../../stores/authStore";
import { dateTime } from "../../utils/format";
import {
  tMessage,
  tPermissionLabel,
  tResource,
  tRoleDescription,
  tRoleName,
  tStatus
} from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

interface RoleFormValues {
  code?: string;
  name: string;
  description?: string;
  permission_codes: string[];
}

export function RolesPage() {
  const [modalOpen, setModalOpen] = useState(false);
  const [editing, setEditing] = useState<RoleDetail | null>(null);
  const [form] = Form.useForm<RoleFormValues>();
  const queryClient = useQueryClient();
  const permissions = useAuthStore((state) => state.permissions);
  const canCreate = hasPermission(permissions, "role:create");
  const canUpdate = hasPermission(permissions, "role:update");
  const canDelete = hasPermission(permissions, "role:delete");

  const rolesQuery = useQuery({
    queryKey: ["admin", "roles"],
    queryFn: listRoles
  });
  const permissionsQuery = useQuery({
    queryKey: ["admin", "permissions"],
    queryFn: listPermissions
  });

  const createMutation = useMutation({
    mutationFn: createRole,
    onSuccess: () => {
      message.success(tMessage("role_created"));
      closeModal();
      queryClient.invalidateQueries({ queryKey: ["admin", "roles"] });
    }
  });
  const updateMutation = useMutation({
    mutationFn: updateRole,
    onSuccess: () => {
      message.success(tMessage("role_updated"));
      closeModal();
      queryClient.invalidateQueries({ queryKey: ["admin", "roles"] });
    }
  });
  const deleteMutation = useMutation({
    mutationFn: deleteRole,
    onSuccess: () => {
      message.success(tMessage("role_deleted"));
      queryClient.invalidateQueries({ queryKey: ["admin", "roles"] });
    }
  });

  const permissionOptions = useMemo(
    () => buildPermissionOptions(permissionsQuery.data?.items ?? []),
    [permissionsQuery.data?.items]
  );

  const openCreate = () => {
    setEditing(null);
    form.setFieldsValue({
      code: "",
      name: "",
      description: "",
      permission_codes: []
    });
    setModalOpen(true);
  };

  const openEdit = (role: RoleDetail) => {
    setEditing(role);
    form.setFieldsValue({
      code: role.code,
      name: role.name,
      description: role.description ?? "",
      permission_codes: role.permission_codes
    });
    setModalOpen(true);
  };

  const closeModal = () => {
    setModalOpen(false);
    setEditing(null);
    form.resetFields();
  };

  const submitRole = (values: RoleFormValues) => {
    const permissionCodes = values.permission_codes ?? [];
    const description = values.description?.trim() || undefined;

    if (editing) {
      const payload: UpdateRolePayload = {
        name: values.name.trim(),
        description,
        permission_codes: permissionCodes
      };
      updateMutation.mutate({ id: editing.id, payload });
      return;
    }

    const payload: CreateRolePayload = {
      code: values.code?.trim() ?? "",
      name: values.name.trim(),
      description,
      permission_codes: permissionCodes
    };
    createMutation.mutate(payload);
  };

  const columns: ColumnsType<RoleDetail> = [
    {
      title: "角色",
      dataIndex: "code",
      key: "code",
      width: 220,
      render: (value: string, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text copyable={{ text: value }} strong>
            {tRoleName(value, record.name)}（{value}）
          </Typography.Text>
          <Typography.Text type="secondary">
            {tRoleDescription(value, record.description)}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "类型",
      dataIndex: "builtin",
      key: "builtin",
      width: 100,
      render: (value: boolean) =>
        value ? <Tag color="blue">{tStatus("builtin")}</Tag> : <Tag>{tStatus("custom")}</Tag>
    },
    {
      title: "权限",
      dataIndex: "permission_codes",
      key: "permission_codes",
      render: (codes: string[]) => renderPermissionCodes(codes)
    },
    {
      title: "更新时间",
      dataIndex: "updated_at",
      key: "updated_at",
      width: 180,
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 120,
      render: (_, record) => (
        <Space size={6}>
          <Button
            size="small"
            icon={<Edit3 size={14} />}
            disabled={record.builtin || !canUpdate}
            onClick={() => openEdit(record)}
          />
          <ConfirmActionButton
            title="删除角色"
            description="删除角色会影响已分配该角色的团队成员权限。"
            okText="删除"
            buttonProps={{
              danger: true,
              size: "small",
              icon: <Trash2 size={14} />,
              disabled: record.builtin || !canDelete
            }}
            loading={deleteMutation.isPending}
            onConfirm={() => deleteMutation.mutate(record.id)}
          >
            删除
          </ConfirmActionButton>
        </Space>
      )
    }
  ];

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>角色权限</Typography.Title>
          <Typography.Text type="secondary">角色、权限集合和内置角色</Typography.Text>
        </div>
        <Space>
          <Button
            icon={<RefreshCw size={16} />}
            onClick={() => {
              rolesQuery.refetch();
              permissionsQuery.refetch();
            }}
          />
          <Button
            type="primary"
            icon={<Plus size={16} />}
            disabled={!canCreate}
            onClick={openCreate}
          >
            新增
          </Button>
        </Space>
      </div>

      {rolesQuery.error ? (
        <Alert type="error" message={tMessage("roles_load_failed")} />
      ) : null}
      {permissionsQuery.error ? (
        <Alert type="error" message={tMessage("permissions_load_failed")} />
      ) : null}
      {createMutation.error ? (
        <Alert type="error" message={tMessage("role_create_failed")} />
      ) : null}
      {updateMutation.error ? (
        <Alert type="error" message={tMessage("role_update_failed")} />
      ) : null}
      {deleteMutation.error ? (
        <Alert type="error" message={tMessage("role_delete_failed")} />
      ) : null}

      <Table
        rowKey="id"
        loading={rolesQuery.isLoading}
        columns={columns}
        dataSource={rolesQuery.data?.items ?? []}
        pagination={false}
        locale={{ emptyText: "暂无数据" }}
      />

      <Modal
        title={editing ? "编辑角色" : "新增角色"}
        open={modalOpen}
        onCancel={closeModal}
        onOk={() => form.submit()}
        confirmLoading={createMutation.isPending || updateMutation.isPending}
        width={820}
        destroyOnClose
      >
        <Form<RoleFormValues> form={form} layout="vertical" onFinish={submitRole}>
          <Form.Item
            name="code"
            label="角色编码"
            rules={[
              { required: !editing, message: "请输入角色编码" },
              {
                pattern: /^[A-Za-z0-9_-]+$/,
                message: "角色编码只能包含字母、数字、_、-"
              }
            ]}
          >
            <Input disabled={Boolean(editing)} />
          </Form.Item>
          <Form.Item
            name="name"
            label="名称"
            rules={[{ required: true, message: "请输入名称" }]}
          >
            <Input maxLength={100} />
          </Form.Item>
          <Form.Item name="description" label="描述">
            <Input.TextArea rows={3} maxLength={300} />
          </Form.Item>
          <Form.Item name="permission_codes" label="权限">
            <Select
              mode="multiple"
              optionFilterProp="label"
              options={permissionOptions}
              loading={permissionsQuery.isLoading}
            />
          </Form.Item>
        </Form>
      </Modal>
    </section>
  );
}

function buildPermissionOptions(permissions: PermissionSummary[]) {
  const groups = new Map<string, PermissionSummary[]>();
  permissions.forEach((permission) => {
    const existing = groups.get(permission.resource) ?? [];
    existing.push(permission);
    groups.set(permission.resource, existing);
  });

  return Array.from(groups.entries()).map(([resource, items]) => ({
    label: tResource(resource),
    options: items.map((permission) => ({
      value: permission.code,
      label: tPermissionLabel(permission, { includeCode: true })
    }))
  }));
}

function renderPermissionCodes(codes: string[]) {
  if (codes.length === 0) {
    return "-";
  }

  const visibleCodes = codes.slice(0, 8);
  return (
    <Space size={[4, 4]} wrap>
      {visibleCodes.map((code) => (
        <Tag key={code}>{tPermissionLabel({ code }, { includeCode: true })}</Tag>
      ))}
      {codes.length > visibleCodes.length ? (
        <Tag>+{codes.length - visibleCodes.length}</Tag>
      ) : null}
    </Space>
  );
}
