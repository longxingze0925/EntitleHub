import {
  Alert,
  Button,
  DatePicker,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Table,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import type { Dayjs } from "dayjs";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Ban, Plus, RefreshCw } from "lucide-react";
import { useState } from "react";

import {
  cancelSubscription,
  createSubscription,
  listApplications,
  listCustomers,
  listSubscriptions,
  type CreateSubscriptionPayload,
  type SubscriptionSummary
} from "../../api/admin";
import { SimplePager } from "../../components/SimplePager";
import { StatusTag } from "../../components/StatusTag";
import { useAuthStore } from "../../stores/authStore";
import { dateTime, shortId } from "../../utils/format";
import { tMessage, tOption } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

const pageSize = 20;

interface CreateSubscriptionFormValues {
  app_id: string;
  customer_id: string;
  plan: string;
  max_devices?: number | null;
  starts_at?: Dayjs;
  expires_at?: Dayjs;
  features?: string[];
}

export function SubscriptionsPage() {
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string | undefined>();
  const [page, setPage] = useState(1);
  const [createOpen, setCreateOpen] = useState(false);
  const [form] = Form.useForm<CreateSubscriptionFormValues>();
  const permissions = useAuthStore((state) => state.permissions);
  const canCreate = hasPermission(permissions, "subscription:create");
  const canCancel = hasPermission(permissions, "subscription:cancel");
  const query = useQuery({
    queryKey: ["admin", "subscriptions", keyword, status, page],
    queryFn: () =>
      listSubscriptions({ keyword, status, page, page_size: pageSize })
  });
  const appsQuery = useQuery({
    queryKey: ["admin", "apps", "active-options"],
    queryFn: () => listApplications({ status: "active" })
  });
  const customersQuery = useQuery({
    queryKey: ["admin", "customers", "active-options"],
    queryFn: () => listCustomers({ status: "active" })
  });
  const createMutation = useMutation({
    mutationFn: createSubscription,
    onSuccess: async () => {
      message.success(tMessage("subscription_created"));
      setCreateOpen(false);
      form.resetFields();
      await query.refetch();
    }
  });
  const cancelMutation = useMutation({
    mutationFn: cancelSubscription,
    onSuccess: async (data) => {
      message.success(tMessage(`subscription_cancelled:${data.revoked_sessions}`));
      await query.refetch();
    }
  });

  const columns: ColumnsType<SubscriptionSummary> = [
    {
      title: "订阅",
      dataIndex: "id",
      key: "id",
      render: (value: string) => shortId(value)
    },
    {
      title: "应用",
      dataIndex: "app_id",
      key: "app_id",
      render: (value: string) => shortId(value)
    },
    {
      title: "客户",
      dataIndex: "customer_id",
      key: "customer_id",
      render: (value: string) => shortId(value)
    },
    {
      title: "套餐",
      dataIndex: "plan",
      key: "plan",
      width: 120,
      render: (value) => <StatusTag value={value} />
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 120,
      render: (value) => <StatusTag value={value} />
    },
    {
      title: "设备上限",
      dataIndex: "max_devices",
      key: "max_devices",
      width: 110
    },
    {
      title: "开始时间",
      dataIndex: "starts_at",
      key: "starts_at",
      render: (value: string) => dateTime(value)
    },
    {
      title: "到期时间",
      dataIndex: "expires_at",
      key: "expires_at",
      render: (value?: string | null) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 120,
      render: (_, record) =>
        canCancel && record.status !== "cancelled" && record.status !== "expired" ? (
          <Popconfirm
            title="取消订阅"
            onConfirm={() => cancelMutation.mutate(record.id)}
          >
            <Button
              size="small"
              icon={<Ban size={14} />}
              loading={cancelMutation.isPending}
            >
              取消
            </Button>
          </Popconfirm>
        ) : null
    }
  ];

  const submitCreate = (values: CreateSubscriptionFormValues) => {
    const payload: CreateSubscriptionPayload = {
      app_id: values.app_id,
      customer_id: values.customer_id,
      plan: values.plan,
      max_devices: cleanNumber(values.max_devices),
      starts_at: values.starts_at?.toISOString(),
      expires_at: values.expires_at?.toISOString(),
      features: cleanFeatures(values.features)
    };
    createMutation.mutate(payload);
  };

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>订阅管理</Typography.Title>
          <Typography.Text type="secondary">
            客户套餐、有效期、设备限制和功能开关
          </Typography.Text>
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
            options={[
              tOption("active"),
              tOption("trialing"),
              tOption("past_due"),
              tOption("cancelled"),
              tOption("expired")
            ]}
            onChange={(value) => {
              setPage(1);
              setStatus(value);
            }}
          />
          <Button
            icon={<RefreshCw size={16} />}
            onClick={() => query.refetch()}
          />
          {canCreate ? (
            <Button
              type="primary"
              icon={<Plus size={16} />}
              onClick={() => setCreateOpen(true)}
            >
              创建订阅
            </Button>
          ) : null}
        </Space>
      </div>
      {query.error ? (
        <Alert type="error" message={tMessage("subscriptions_load_failed")} />
      ) : null}
      {createMutation.error ? (
        <Alert type="error" message={tMessage("subscription_create_failed")} />
      ) : null}
      {cancelMutation.error ? (
        <Alert type="error" message={tMessage("subscription_cancel_failed")} />
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
        title="创建订阅"
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={createMutation.isPending}
        destroyOnClose
      >
        <Form<CreateSubscriptionFormValues>
          form={form}
          layout="vertical"
          onFinish={submitCreate}
          initialValues={{ plan: "pro", max_devices: 1 }}
        >
          <Form.Item
            name="app_id"
            label="应用"
            rules={[{ required: true, message: "请选择应用" }]}
          >
            <Select
              showSearch
              optionFilterProp="label"
              loading={appsQuery.isLoading}
              options={(appsQuery.data?.items ?? []).map((app) => ({
                value: app.id,
                label: `${app.name} · ${shortId(app.id)}`
              }))}
            />
          </Form.Item>
          <Form.Item
            name="customer_id"
            label="客户"
            rules={[{ required: true, message: "请选择客户" }]}
          >
            <Select
              showSearch
              optionFilterProp="label"
              loading={customersQuery.isLoading}
              options={(customersQuery.data?.items ?? []).map((customer) => ({
                value: customer.id,
                label: `${customer.name ?? customer.email} · ${customer.email}`
              }))}
            />
          </Form.Item>
          <Form.Item
            name="plan"
            label="套餐"
            rules={[{ required: true, message: "请输入套餐" }]}
          >
            <Input placeholder="pro" />
          </Form.Item>
          <Form.Item name="max_devices" label="设备上限">
            <InputNumber min={0} precision={0} className="form-number" />
          </Form.Item>
          <Form.Item name="starts_at" label="开始时间">
            <DatePicker showTime className="form-date" />
          </Form.Item>
          <Form.Item name="expires_at" label="到期时间">
            <DatePicker showTime className="form-date" />
          </Form.Item>
          <Form.Item name="features" label="功能标记">
            <Select mode="tags" tokenSeparators={[","]} />
          </Form.Item>
        </Form>
      </Modal>
    </section>
  );
}

function cleanNumber(value?: number | null): number | undefined {
  return typeof value === "number" ? value : undefined;
}

function cleanFeatures(values?: string[]): string[] | undefined {
  const features = values
    ?.map((value) => value.trim())
    .filter((value) => value.length > 0);

  return features && features.length > 0 ? features : undefined;
}
