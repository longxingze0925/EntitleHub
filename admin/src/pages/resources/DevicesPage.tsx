import {
  Alert,
  Button,
  Form,
  Input,
  Modal,
  Popconfirm,
  Select,
  Space,
  Table,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Ban, RefreshCw, Unlink, Unlock } from "lucide-react";
import { useState } from "react";

import {
  blacklistDevice,
  listDevices,
  unbindDevice,
  unblacklistDevice,
  type DeviceSummary
} from "../../api/admin";
import { ConfirmActionButton } from "../../components/ConfirmActionButton";
import { HistoryToggle } from "../../components/HistoryToggle";
import { SimplePager } from "../../components/SimplePager";
import { StatusTag } from "../../components/StatusTag";
import { useAuthStore } from "../../stores/authStore";
import { dateTime, shortId } from "../../utils/format";
import { tMessage, tOption } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

const pageSize = 20;

interface BlacklistFormValues {
  reason: string;
}

export function DevicesPage() {
  const [machineId, setMachineId] = useState("");
  const [status, setStatus] = useState<string | undefined>();
  const [includeHistory, setIncludeHistory] = useState(false);
  const [page, setPage] = useState(1);
  const [blacklistTarget, setBlacklistTarget] = useState<DeviceSummary | null>(null);
  const [form] = Form.useForm<BlacklistFormValues>();
  const permissions = useAuthStore((state) => state.permissions);
  const canUnbind = hasPermission(permissions, "device:unbind");
  const canBlacklist = hasPermission(permissions, "device:blacklist");
  const canUnblacklist = hasPermission(permissions, "device:unblacklist");
  const query = useQuery({
    queryKey: ["admin", "devices", machineId, status, includeHistory, page],
    queryFn: () =>
      listDevices({
        machine_id: machineId,
        status,
        include_history: includeHistory,
        page,
        page_size: pageSize
      })
  });
  const unbindMutation = useMutation({
    mutationFn: unbindDevice,
    onSuccess: async () => {
      message.success(tMessage("device_unbound"));
      await query.refetch();
    }
  });
  const blacklistMutation = useMutation({
    mutationFn: blacklistDevice,
    onSuccess: async () => {
      message.success(tMessage("device_blacklisted"));
      setBlacklistTarget(null);
      form.resetFields();
      await query.refetch();
    }
  });
  const unblacklistMutation = useMutation({
    mutationFn: unblacklistDevice,
    onSuccess: async () => {
      message.success(tMessage("device_unblacklisted"));
      await query.refetch();
    }
  });

  const columns: ColumnsType<DeviceSummary> = [
    {
      title: "设备",
      dataIndex: "machine_id",
      key: "machine_id",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text strong>{record.device_name ?? record.machine_id}</Typography.Text>
          <Typography.Text type="secondary">{record.machine_id}</Typography.Text>
        </Space>
      )
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
      title: "系统",
      dataIndex: "os",
      key: "os",
      render: (value?: string | null) => value ?? "-"
    },
    {
      title: "版本",
      dataIndex: "app_version",
      key: "app_version",
      render: (value?: string | null) => value ?? "-"
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 120,
      render: (value) => <StatusTag value={value} />
    },
    {
      title: "最后心跳",
      dataIndex: "last_seen_at",
      key: "last_seen_at",
      render: (value?: string | null) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 230,
      render: (_, record) => (
        <Space>
          {canUnbind && record.status !== "unbound" ? (
            <ConfirmActionButton
              title="解绑设备"
              description="解绑会撤销该设备相关会话，客户端需要重新绑定或重新激活。"
              buttonProps={{
                size: "small",
                icon: <Unlink size={14} />
              }}
              loading={unbindMutation.isPending}
              onConfirm={() => unbindMutation.mutate(record.id)}
            >
              解绑
            </ConfirmActionButton>
          ) : null}
          {canBlacklist && record.status !== "blacklisted" && record.status !== "unbound" ? (
            <Button
              size="small"
              icon={<Ban size={14} />}
              onClick={() => setBlacklistTarget(record)}
            >
              拉黑
            </Button>
          ) : null}
          {canUnblacklist && record.status === "blacklisted" ? (
            <Popconfirm
              title="解除拉黑"
              onConfirm={() => unblacklistMutation.mutate(record.id)}
            >
              <Button
                size="small"
                icon={<Unlock size={14} />}
                loading={unblacklistMutation.isPending}
              >
                解除
              </Button>
            </Popconfirm>
          ) : null}
        </Space>
      )
    }
  ];

  const submitBlacklist = (values: BlacklistFormValues) => {
    const reason = clean(values.reason);
    if (!blacklistTarget || !reason) {
      return;
    }

    blacklistMutation.mutate({
      id: blacklistTarget.id,
      reason
    });
  };

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>设备管理</Typography.Title>
          <Typography.Text type="secondary">绑定设备、会话状态和拉黑控制</Typography.Text>
        </div>
        <Space>
          <Input.Search
            allowClear
            placeholder="机器 ID"
            onSearch={(value) => {
              setPage(1);
              setMachineId(value);
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
              tOption("disabled"),
              tOption("blacklisted"),
              tOption("unbound")
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
        </Space>
      </div>
      {query.error ? (
        <Alert type="error" message={tMessage("devices_load_failed")} />
      ) : null}
      {unbindMutation.error ||
      blacklistMutation.error ||
      unblacklistMutation.error ? (
        <Alert type="error" message={tMessage("device_status_update_failed")} />
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
        title="拉黑设备"
        open={Boolean(blacklistTarget)}
        onCancel={() => {
          setBlacklistTarget(null);
          form.resetFields();
        }}
        onOk={() => form.submit()}
        confirmLoading={blacklistMutation.isPending}
        destroyOnClose
      >
        <Form<BlacklistFormValues>
          form={form}
          layout="vertical"
          onFinish={submitBlacklist}
        >
          <Form.Item label="设备">
            <Typography.Text>{blacklistTarget?.machine_id}</Typography.Text>
          </Form.Item>
          <Form.Item
            name="reason"
            label="原因"
            rules={[
              { required: true, whitespace: true, message: "请输入拉黑原因" },
              { max: 500, message: "原因不能超过 500 字" }
            ]}
          >
            <Input.TextArea rows={3} maxLength={500} showCount />
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
