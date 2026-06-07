import {
  Alert,
  Button,
  Form,
  Input,
  Modal,
  Select,
  Space,
  Table,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Ban, Edit3, KeyRound, Plus, RefreshCw } from "lucide-react";
import { useState } from "react";

import {
  createCustomer,
  disableCustomer,
  listCustomers,
  resetCustomerPassword,
  updateCustomer,
  type CreateCustomerPayload,
  type Customer,
  type UpdateCustomerPayload
} from "../../api/admin";
import { ConfirmActionButton } from "../../components/ConfirmActionButton";
import { HistoryToggle } from "../../components/HistoryToggle";
import { SimplePager } from "../../components/SimplePager";
import { StatusTag } from "../../components/StatusTag";
import { useAuthStore } from "../../stores/authStore";
import { tMessage, tOption } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

const pageSize = 20;

export function CustomersPage() {
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string | undefined>();
  const [includeHistory, setIncludeHistory] = useState(false);
  const [page, setPage] = useState(1);
  const [createOpen, setCreateOpen] = useState(false);
  const [editingCustomer, setEditingCustomer] = useState<Customer | null>(null);
  const [createForm] = Form.useForm<CreateCustomerPayload>();
  const [editForm] = Form.useForm<UpdateCustomerPayload>();
  const permissions = useAuthStore((state) => state.permissions);
  const canCreate = hasPermission(permissions, "customer:create");
  const canUpdate = hasPermission(permissions, "customer:update");
  const canDisable = hasPermission(permissions, "customer:disable");
  const canResetPassword = hasPermission(permissions, "customer:reset_password");
  const query = useQuery({
    queryKey: ["admin", "customers", keyword, status, includeHistory, page],
    queryFn: () =>
      listCustomers({
        keyword,
        status,
        include_history: includeHistory,
        page,
        page_size: pageSize
      })
  });
  const createMutation = useMutation({
    mutationFn: createCustomer,
    onSuccess: async () => {
      message.success(tMessage("customer_created"));
      setCreateOpen(false);
      createForm.resetFields();
      await query.refetch();
    }
  });
  const updateMutation = useMutation({
    mutationFn: updateCustomer,
    onSuccess: async () => {
      message.success(tMessage("customer_updated"));
      setEditingCustomer(null);
      editForm.resetFields();
      await query.refetch();
    }
  });
  const disableMutation = useMutation({
    mutationFn: disableCustomer,
    onSuccess: async (data) => {
      message.success(tMessage(`customer_disabled:${data.revoked_sessions ?? 0}`));
      await query.refetch();
    }
  });
  const resetPasswordMutation = useMutation({
    mutationFn: resetCustomerPassword,
    onSuccess: () => {
      message.success(tMessage("customer_password_reset_email_queued"));
    }
  });

  const openEdit = (customer: Customer) => {
    setEditingCustomer(customer);
    editForm.setFieldsValue({
      name: customer.name ?? undefined,
      phone: customer.phone ?? undefined,
      company: customer.company ?? undefined,
      remark: customer.remark ?? undefined
    });
  };

  const columns: ColumnsType<Customer> = [
    {
      title: "客户",
      dataIndex: "email",
      key: "email",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text strong>{record.name ?? record.email}</Typography.Text>
          <Typography.Text type="secondary">{record.email}</Typography.Text>
        </Space>
      )
    },
    {
      title: "公司",
      dataIndex: "company",
      key: "company",
      render: (value?: string | null) => value ?? "-"
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 110,
      render: (value) => <StatusTag value={value} />
    },
    {
      title: "邮箱验证",
      dataIndex: "email_verified",
      key: "email_verified",
      width: 110,
      render: (verified: boolean) => (verified ? "已验证" : "待验证")
    },
    {
      title: "操作",
      key: "actions",
      width: 240,
      render: (_, record) => (
        <Space>
          {canUpdate ? (
            <Button size="small" icon={<Edit3 size={14} />} onClick={() => openEdit(record)}>
              编辑
            </Button>
          ) : null}
          {canDisable && record.status !== "disabled" ? (
            <ConfirmActionButton
              title="禁用客户"
              description="禁用后会撤销该客户相关客户端会话，客户将无法继续登录使用。"
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
          {canResetPassword ? (
            <ConfirmActionButton
              title="重置密码"
              description="系统会向客户邮箱发送密码重置邮件，已有会话可能在重置后失效。"
              buttonProps={{
                size: "small",
                icon: <KeyRound size={14} />
              }}
              loading={resetPasswordMutation.isPending}
              onConfirm={() => resetPasswordMutation.mutate(record.id)}
            >
              重置
            </ConfirmActionButton>
          ) : null}
        </Space>
      )
    }
  ];

  const submitCreate = (values: CreateCustomerPayload) => {
    const payload: CreateCustomerPayload = {
      email: values.email.trim(),
      name: clean(values.name),
      password: clean(values.password),
      phone: clean(values.phone),
      company: clean(values.company),
      remark: clean(values.remark)
    };
    createMutation.mutate(payload);
  };

  const submitUpdate = (values: UpdateCustomerPayload) => {
    if (!editingCustomer) {
      return;
    }

    updateMutation.mutate({
      id: editingCustomer.id,
      payload: {
        name: clean(values.name),
        phone: clean(values.phone),
        company: clean(values.company),
        remark: clean(values.remark)
      }
    });
  };

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>客户管理</Typography.Title>
          <Typography.Text type="secondary">软件客户、状态和重置操作</Typography.Text>
        </div>
        <Space>
          <Input.Search
            allowClear
            placeholder="关键词"
            onSearch={(value) => {
              setPage(1);
              setKeyword(value);
            }}
            className="table-search"
          />
          <Select
            allowClear
            placeholder="状态"
            className="table-filter"
            value={status}
            options={[tOption("active"), tOption("disabled")]}
            onChange={(value) => {
              setPage(1);
              setStatus(value);
            }}
          />
          <HistoryToggle
            checked={includeHistory}
            onChange={(checked) => {
              setPage(1);
              setIncludeHistory(checked);
            }}
          />
          <Button icon={<RefreshCw size={16} />} onClick={() => query.refetch()} />
          {canCreate ? (
            <Button
              type="primary"
              icon={<Plus size={16} />}
              onClick={() => setCreateOpen(true)}
            >
              创建客户
            </Button>
          ) : null}
        </Space>
      </div>
      {query.error ? (
        <Alert type="error" message={tMessage("customers_load_failed")} />
      ) : null}
      {createMutation.error ? (
        <Alert type="error" message={tMessage("customer_create_failed")} />
      ) : null}
      {updateMutation.error ? (
        <Alert type="error" message={tMessage("customer_update_failed")} />
      ) : null}
      {disableMutation.error ? (
        <Alert type="error" message={tMessage("customer_disable_failed")} />
      ) : null}
      {resetPasswordMutation.error ? (
        <Alert type="error" message={tMessage("customer_password_reset_failed")} />
      ) : null}
      <Table
        rowKey="id"
        loading={query.isLoading}
        columns={columns}
        dataSource={query.data?.items ?? []}
        pagination={false}
        locale={{ emptyText: "暂无数据" }}
      />
      <SimplePager
        page={page}
        pageSize={pageSize}
        itemCount={query.data?.items.length ?? 0}
        loading={query.isFetching}
        onChange={setPage}
      />

      <Modal
        title="创建客户"
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onOk={() => createForm.submit()}
        confirmLoading={createMutation.isPending}
        destroyOnClose
      >
        <Form<CreateCustomerPayload>
          form={createForm}
          layout="vertical"
          onFinish={submitCreate}
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
          <Form.Item name="name" label="姓名">
            <Input />
          </Form.Item>
          <Form.Item name="password" label="初始密码">
            <Input.Password autoComplete="new-password" />
          </Form.Item>
          <Form.Item name="phone" label="电话">
            <Input />
          </Form.Item>
          <Form.Item name="company" label="公司">
            <Input />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={3} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="编辑客户"
        open={Boolean(editingCustomer)}
        onCancel={() => {
          setEditingCustomer(null);
          editForm.resetFields();
        }}
        onOk={() => editForm.submit()}
        confirmLoading={updateMutation.isPending}
        destroyOnClose
      >
        <Form<UpdateCustomerPayload>
          form={editForm}
          layout="vertical"
          onFinish={submitUpdate}
        >
          <Form.Item name="name" label="姓名">
            <Input />
          </Form.Item>
          <Form.Item name="phone" label="电话">
            <Input />
          </Form.Item>
          <Form.Item name="company" label="公司">
            <Input />
          </Form.Item>
          <Form.Item name="remark" label="备注">
            <Input.TextArea rows={3} />
          </Form.Item>
        </Form>
      </Modal>
    </section>
  );
}

function clean(value?: string): string | undefined {
  const trimmed = value?.trim();

  return trimmed ? trimmed : undefined;
}
