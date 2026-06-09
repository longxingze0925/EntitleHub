import {
  Alert,
  Button,
  DatePicker,
  Form,
  Input,
  InputNumber,
  Modal,
  Select,
  Space,
  Table,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import type { Dayjs } from "dayjs";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Ban, CalendarPlus, Pause, Plus, RefreshCw, RotateCcw } from "lucide-react";
import { useState } from "react";

import {
  createLicense,
  listApplications,
  listCustomers,
  listLicenses,
  renewLicense,
  resetLicenseDevices,
  revokeLicense,
  suspendLicense,
  type CreateLicensePayload,
  type CreateLicenseResult,
  type LicenseSummary
} from "../../api/admin";
import { ConfirmActionButton } from "../../components/ConfirmActionButton";
import { HistoryToggle } from "../../components/HistoryToggle";
import { SimplePager } from "../../components/SimplePager";
import { StatusTag } from "../../components/StatusTag";
import { useAuthStore } from "../../stores/authStore";
import { dateTime, shortId } from "../../utils/format";
import {
  effectiveTemporalStatus,
  tMessage,
  tOption,
  tStatus
} from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

const pageSize = 20;

interface CreateLicenseFormValues {
  app_id: string;
  customer_id?: string;
  type?: string;
  max_devices?: number | null;
  expires_at?: Dayjs;
  features?: string[];
}

interface RenewLicenseFormValues {
  expires_at?: Dayjs;
}

interface ResetDevicesFormValues {
  reason?: string;
}

export function LicensesPage() {
  const [keyword, setKeyword] = useState("");
  const [status, setStatus] = useState<string | undefined>();
  const [includeHistory, setIncludeHistory] = useState(false);
  const [page, setPage] = useState(1);
  const [createOpen, setCreateOpen] = useState(false);
  const [renewTarget, setRenewTarget] = useState<LicenseSummary | null>(null);
  const [resetDevicesTarget, setResetDevicesTarget] =
    useState<LicenseSummary | null>(null);
  const [createdLicense, setCreatedLicense] =
    useState<CreateLicenseResult | null>(null);
  const [form] = Form.useForm<CreateLicenseFormValues>();
  const [renewForm] = Form.useForm<RenewLicenseFormValues>();
  const [resetDevicesForm] = Form.useForm<ResetDevicesFormValues>();
  const permissions = useAuthStore((state) => state.permissions);
  const canCreate = hasPermission(permissions, "license:create");
  const canRevoke = hasPermission(permissions, "license:revoke");
  const canSuspend = hasPermission(permissions, "license:suspend");
  const canRenew = hasPermission(permissions, "license:renew");
  const canResetDevice = hasPermission(permissions, "license:reset_device");
  const query = useQuery({
    queryKey: ["admin", "licenses", keyword, status, includeHistory, page],
    queryFn: () =>
      listLicenses({
        keyword,
        status,
        include_history: includeHistory,
        page,
        page_size: pageSize
      })
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
    mutationFn: createLicense,
    onSuccess: async (data) => {
      message.success(tMessage("license_created"));
      setCreatedLicense(data);
      setCreateOpen(false);
      form.resetFields();
      await query.refetch();
    }
  });
  const revokeMutation = useMutation({
    mutationFn: revokeLicense,
    onSuccess: async (data) => {
      message.success(tMessage(`license_revoked:${data.revoked_sessions}`));
      await query.refetch();
    }
  });
  const suspendMutation = useMutation({
    mutationFn: suspendLicense,
    onSuccess: async (data) => {
      message.success(tMessage(`license_suspended:${data.revoked_sessions}`));
      await query.refetch();
    }
  });
  const renewMutation = useMutation({
    mutationFn: renewLicense,
    onSuccess: async () => {
      message.success(tMessage("license_renewed"));
      setRenewTarget(null);
      renewForm.resetFields();
      await query.refetch();
    }
  });
  const resetDevicesMutation = useMutation({
    mutationFn: resetLicenseDevices,
    onSuccess: async (data) => {
      message.success(tMessage(`license_devices_reset:${data.revoked_sessions}`));
      setResetDevicesTarget(null);
      resetDevicesForm.resetFields();
      await query.refetch();
    }
  });

  const columns: ColumnsType<LicenseSummary> = [
    {
      title: "授权",
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
      render: (value?: string | null) => shortId(value)
    },
    {
      title: "类型",
      dataIndex: "type",
      key: "type",
      width: 110,
      render: (value) => <StatusTag value={value} />
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 110,
      render: (_, record) => (
        <StatusTag value={effectiveTemporalStatus(record)} />
      )
    },
    {
      title: "设备上限",
      dataIndex: "max_devices",
      key: "max_devices",
      width: 110
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
      width: 420,
      render: (_, record) => {
        const effectiveStatus = effectiveTemporalStatus(record);

        return (
          <Space className="table-actions-nowrap">
            {canRevoke && record.status !== "revoked" ? (
              <ConfirmActionButton
                title="吊销授权"
                description="吊销后该授权不可继续使用，并会撤销相关客户端会话。"
                buttonProps={{
                  size: "small",
                  icon: <Ban size={14} />
                }}
                loading={revokeMutation.isPending}
                onConfirm={() => revokeMutation.mutate(record.id)}
              >
                吊销
              </ConfirmActionButton>
            ) : null}
            {canSuspend && effectiveStatus === "active" ? (
              <ConfirmActionButton
                title="暂停授权"
                description="暂停后客户端会立即失去该授权的可用状态，并撤销相关会话。"
                buttonProps={{
                  size: "small",
                  icon: <Pause size={14} />
                }}
                loading={suspendMutation.isPending}
                onConfirm={() => suspendMutation.mutate(record.id)}
              >
                暂停
              </ConfirmActionButton>
            ) : null}
            {canRenew && record.status !== "revoked" ? (
              <Button
                size="small"
                icon={<CalendarPlus size={14} />}
                onClick={() => setRenewTarget(record)}
              >
                续期
              </Button>
            ) : null}
            {canResetDevice && record.status !== "revoked" ? (
              <Button
                size="small"
                icon={<RotateCcw size={14} />}
                onClick={() => setResetDevicesTarget(record)}
              >
                重置设备
              </Button>
            ) : null}
          </Space>
        );
      }
    }
  ];

  const submitCreate = (values: CreateLicenseFormValues) => {
    const payload: CreateLicensePayload = {
      app_id: values.app_id,
      customer_id: values.customer_id,
      type: values.type,
      max_devices: cleanNumber(values.max_devices),
      expires_at: values.expires_at?.toISOString(),
      features: cleanFeatures(values.features)
    };
    createMutation.mutate(payload);
  };

  const submitRenew = (values: RenewLicenseFormValues) => {
    if (!renewTarget || !values.expires_at) {
      return;
    }

    renewMutation.mutate({
      id: renewTarget.id,
      expires_at: values.expires_at.toISOString()
    });
  };

  const submitResetDevices = (values: ResetDevicesFormValues) => {
    const reason = clean(values.reason);
    if (!resetDevicesTarget || !reason) {
      return;
    }

    resetDevicesMutation.mutate({
      id: resetDevicesTarget.id,
      reason
    });
  };

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>授权管理</Typography.Title>
          <Typography.Text type="secondary">授权码、有效期、设备限制和功能开关</Typography.Text>
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
            options={[
              tOption("active"),
              tOption("suspended"),
              tOption("revoked"),
              tOption("expired")
            ]}
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
              创建授权
            </Button>
          ) : null}
        </Space>
      </div>
      {query.error ? (
        <Alert type="error" message={tMessage("licenses_load_failed")} />
      ) : null}
      {createMutation.error ? (
        <Alert type="error" message={tMessage("license_create_failed")} />
      ) : null}
      {revokeMutation.error ||
      suspendMutation.error ||
      renewMutation.error ||
      resetDevicesMutation.error ? (
        <Alert type="error" message={tMessage("license_status_update_failed")} />
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
        title="创建授权"
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onOk={() => form.submit()}
        confirmLoading={createMutation.isPending}
        destroyOnClose
      >
        <Form<CreateLicenseFormValues>
          form={form}
          layout="vertical"
          onFinish={submitCreate}
          initialValues={{ type: "standard", max_devices: 1 }}
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
          <Form.Item name="customer_id" label="客户">
            <Select
              allowClear
              showSearch
              optionFilterProp="label"
              loading={customersQuery.isLoading}
              options={(customersQuery.data?.items ?? []).map((customer) => ({
                value: customer.id,
                label: `${customer.name ?? customer.email} · ${customer.email}`
              }))}
            />
          </Form.Item>
          <Form.Item name="type" label="授权类型">
            <Select
              options={[
                tOption("standard"),
                tOption("trial"),
                tOption("enterprise")
              ]}
            />
          </Form.Item>
          <Form.Item name="max_devices" label="设备上限">
            <InputNumber min={0} precision={0} className="form-number" />
          </Form.Item>
          <Form.Item name="expires_at" label="到期时间">
            <DatePicker showTime className="form-date" />
          </Form.Item>
          <Form.Item name="features" label="功能标记">
            <Select mode="tags" tokenSeparators={[","]} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="授权码"
        open={Boolean(createdLicense)}
        onCancel={() => setCreatedLicense(null)}
        onOk={() => setCreatedLicense(null)}
      >
        <Space direction="vertical" size={12} className="token-result">
          <Typography.Text type="secondary">授权码（license_key）</Typography.Text>
          <Typography.Text copyable>{createdLicense?.license_key}</Typography.Text>
          <Typography.Text type="secondary">授权 ID（license_id）</Typography.Text>
          <Typography.Text copyable>{createdLicense?.license.id}</Typography.Text>
          <Typography.Text type="secondary">
            {createdLicense
              ? `${tStatus(createdLicense.license.status)} · 到期时间 ${dateTime(
                  createdLicense.license.expires_at
                )}`
              : null}
          </Typography.Text>
        </Space>
      </Modal>

      <Modal
        title="续期授权"
        open={Boolean(renewTarget)}
        onCancel={() => {
          setRenewTarget(null);
          renewForm.resetFields();
        }}
        onOk={() => renewForm.submit()}
        confirmLoading={renewMutation.isPending}
        destroyOnClose
      >
        <Form<RenewLicenseFormValues>
          form={renewForm}
          layout="vertical"
          onFinish={submitRenew}
        >
          <Form.Item label="授权">
            <Typography.Text>{shortId(renewTarget?.id)}</Typography.Text>
          </Form.Item>
          <Form.Item
            name="expires_at"
            label="新的到期时间"
            rules={[{ required: true, message: "请选择新的到期时间" }]}
          >
            <DatePicker showTime className="form-date" />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="重置设备"
        open={Boolean(resetDevicesTarget)}
        onCancel={() => {
          setResetDevicesTarget(null);
          resetDevicesForm.resetFields();
        }}
        onOk={() => resetDevicesForm.submit()}
        confirmLoading={resetDevicesMutation.isPending}
        destroyOnClose
      >
        <Form<ResetDevicesFormValues>
          form={resetDevicesForm}
          layout="vertical"
          onFinish={submitResetDevices}
        >
          <Form.Item label="授权">
            <Typography.Text>{shortId(resetDevicesTarget?.id)}</Typography.Text>
          </Form.Item>
          <Form.Item
            name="reason"
            label="原因"
            rules={[{ required: true, message: "请输入重置原因" }]}
          >
            <Input.TextArea rows={3} />
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

function clean(value?: string): string | undefined {
  const trimmed = value?.trim();

  return trimmed ? trimmed : undefined;
}
