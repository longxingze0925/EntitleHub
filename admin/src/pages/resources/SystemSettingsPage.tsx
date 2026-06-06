import {
  Alert,
  Button,
  Form,
  Input,
  Modal,
  Space,
  Table,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Pencil, Plus, RefreshCw } from "lucide-react";
import { useState } from "react";

import {
  listSystemSettings,
  updateSystemSetting,
  type SystemSetting
} from "../../api/admin";
import { useAuthStore } from "../../stores/authStore";
import { dateTime } from "../../utils/format";
import { tMessage } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

interface SettingFormValues {
  key: string;
  value: string;
}

const defaultValue = `{
  "enabled": true
}`;

export function SystemSettingsPage() {
  const [form] = Form.useForm<SettingFormValues>();
  const [editing, setEditing] = useState<SystemSetting | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  const queryClient = useQueryClient();
  const permissions = useAuthStore((state) => state.permissions);
  const canUpdate = hasPermission(permissions, "system:update");

  const query = useQuery({
    queryKey: ["admin", "system-settings"],
    queryFn: listSystemSettings
  });

  const mutation = useMutation({
    mutationFn: (values: SettingFormValues) =>
      updateSystemSetting({
        key: values.key.trim(),
        payload: { value: JSON.parse(values.value) }
      }),
    onSuccess: () => {
      message.success(tMessage("system_setting_saved"));
      setModalOpen(false);
      setEditing(null);
      form.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin", "system-settings"] });
    }
  });

  const openCreate = () => {
    setEditing(null);
    form.setFieldsValue({ key: "", value: defaultValue });
    setModalOpen(true);
  };

  const openEdit = (setting: SystemSetting) => {
    setEditing(setting);
    form.setFieldsValue({
      key: setting.key,
      value: stringifyJson(setting.value)
    });
    setModalOpen(true);
  };

  const columns: ColumnsType<SystemSetting> = [
    {
      title: "配置键",
      dataIndex: "key",
      key: "key",
      width: 240,
      render: (value: string) => <Typography.Text copyable>{value}</Typography.Text>
    },
    {
      title: "配置值",
      dataIndex: "value",
      key: "value",
      render: (value: unknown) => (
        <pre className="json-view setting-json-view">{stringifyJson(value)}</pre>
      )
    },
    {
      title: "更新时间",
      dataIndex: "updated_at",
      key: "updated_at",
      width: 190,
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 90,
      render: (_, record) => (
        <Button
          size="small"
          icon={<Pencil size={14} />}
          disabled={!canUpdate}
          onClick={() => openEdit(record)}
        />
      )
    }
  ];

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>系统配置</Typography.Title>
          <Typography.Text type="secondary">全局运行配置</Typography.Text>
        </div>
        <Space>
          <Button icon={<RefreshCw size={16} />} onClick={() => query.refetch()} />
          <Button
            type="primary"
            icon={<Plus size={16} />}
            disabled={!canUpdate}
            onClick={openCreate}
          >
            新增
          </Button>
        </Space>
      </div>

      {query.error ? (
        <Alert type="error" message={tMessage("system_settings_load_failed")} />
      ) : null}
      {mutation.error ? (
        <Alert type="error" message={tMessage("system_setting_save_failed")} />
      ) : null}

      <Table
        rowKey="key"
        loading={query.isLoading}
        columns={columns}
        dataSource={query.data?.items ?? []}
        pagination={false}
        locale={{ emptyText: "暂无数据" }}
      />

      <Modal
        title={editing ? "编辑配置" : "新增配置"}
        open={modalOpen}
        onCancel={() => {
          setModalOpen(false);
          setEditing(null);
        }}
        onOk={() => form.submit()}
        confirmLoading={mutation.isPending}
        width={760}
        destroyOnClose
      >
        <Form<SettingFormValues>
          form={form}
          layout="vertical"
          onFinish={(values) => mutation.mutate(values)}
        >
          <Form.Item
            name="key"
            label="配置键"
            rules={[
              { required: true, message: "请输入配置键" },
              {
                pattern: /^[A-Za-z0-9_.:-]+$/,
                message: "配置键只能包含字母、数字、_、-、.、:"
              }
            ]}
          >
            <Input disabled={Boolean(editing)} />
          </Form.Item>
          <Form.Item
            name="value"
            label="配置值"
            rules={[
              { required: true, message: "请输入 JSON" },
              {
                validator: (_, value) => {
                  try {
                    JSON.parse(value);
                    return Promise.resolve();
                  } catch {
                    return Promise.reject(new Error("JSON 格式不正确"));
                  }
                }
              }
            ]}
          >
            <Input.TextArea className="settings-json-editor" rows={12} />
          </Form.Item>
        </Form>
      </Modal>
    </section>
  );
}

function stringifyJson(value: unknown): string {
  return JSON.stringify(value, null, 2);
}
