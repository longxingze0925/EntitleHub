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
  Tag,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import type { Dayjs } from "dayjs";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Archive, Edit3, Plus, RefreshCw, Rocket } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import {
  createSecureScript,
  deprecateSecureScript,
  listApplications,
  listSecureScripts,
  publishSecureScript,
  updateSecureScriptContent,
  type SecureScriptSummary
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

interface CreateScriptFormValues {
  name: string;
  version: string;
  version_code?: number | null;
  required_features?: string[];
  expires_at?: Dayjs;
  content?: string;
}

interface UpdateContentFormValues {
  content: string;
  version?: string;
  version_code?: number | null;
}

export function ScriptsPage() {
  const [appId, setAppId] = useState<string>();
  const [status, setStatus] = useState<string | undefined>();
  const [includeHistory, setIncludeHistory] = useState(false);
  const [page, setPage] = useState(1);
  const [createOpen, setCreateOpen] = useState(false);
  const [editingScript, setEditingScript] = useState<SecureScriptSummary | null>(null);
  const [signedScript, setSignedScript] = useState<SecureScriptSummary | null>(null);
  const [createForm] = Form.useForm<CreateScriptFormValues>();
  const [updateForm] = Form.useForm<UpdateContentFormValues>();
  const permissions = useAuthStore((state) => state.permissions);
  const canCreate = hasPermission(permissions, "script:create");
  const canUpdate = hasPermission(permissions, "script:update");
  const canPublish = hasPermission(permissions, "script:publish");
  const canDeprecate = hasPermission(permissions, "script:deprecate");
  const appsQuery = useQuery({
    queryKey: ["admin", "apps", "script-selector", includeHistory],
    queryFn: () => listApplications({ include_history: includeHistory })
  });
  const appOptions = useMemo(
    () =>
      (appsQuery.data?.items ?? []).map((app) => ({
        value: app.id,
        label: app.name
      })),
    [appsQuery.data?.items]
  );

  useEffect(() => {
    const currentVisible = appOptions.some((option) => option.value === appId);
    if (appOptions.length > 0 && (!appId || !currentVisible)) {
      setAppId(appOptions[0].value);
    }
  }, [appId, appOptions]);

  const scriptsQuery = useQuery({
    queryKey: ["admin", "secure-scripts", appId, status, includeHistory, page],
    queryFn: () =>
      listSecureScripts({
        appId: appId!,
        status,
        include_history: includeHistory,
        page,
        page_size: pageSize
      }),
    enabled: Boolean(appId)
  });
  const createMutation = useMutation({
    mutationFn: async (values: CreateScriptFormValues) => {
      if (!appId) {
        throw new Error("application_required");
      }

      const created = await createSecureScript({
        appId,
        payload: {
          name: values.name.trim(),
          version: values.version.trim(),
          version_code: requiredNumber(values.version_code),
          required_features: cleanFeatures(values.required_features),
          expires_at: values.expires_at?.toISOString()
        }
      });

      if (canUpdate && values.content !== undefined && values.content.length > 0) {
        const updated = await updateSecureScriptContent({
          id: created.script.id,
          payload: {
            content_base64: encodeBase64(values.content)
          }
        });

        return updated.script;
      }

      return created.script;
    },
    onSuccess: async (script) => {
      message.success(tMessage("secure_script_created"));
      setSignedScript(script);
      setCreateOpen(false);
      createForm.resetFields();
      await scriptsQuery.refetch();
    }
  });
  const updateMutation = useMutation({
    mutationFn: async (values: UpdateContentFormValues) => {
      if (!editingScript) {
        throw new Error("secure_script_required");
      }

      const updated = await updateSecureScriptContent({
        id: editingScript.id,
        payload: {
          content_base64: encodeBase64(values.content),
          version: clean(values.version),
          version_code: cleanNumber(values.version_code)
        }
      });

      return updated.script;
    },
    onSuccess: async (script) => {
      message.success(tMessage("secure_script_content_updated"));
      setSignedScript(script);
      setEditingScript(null);
      updateForm.resetFields();
      await scriptsQuery.refetch();
    }
  });
  const publishMutation = useMutation({
    mutationFn: publishSecureScript,
    onSuccess: async () => {
      message.success(tMessage("secure_script_published"));
      await scriptsQuery.refetch();
    }
  });
  const deprecateMutation = useMutation({
    mutationFn: deprecateSecureScript,
    onSuccess: async () => {
      message.success(tMessage("secure_script_deprecated"));
      await scriptsQuery.refetch();
    }
  });

  const openUpdate = (script: SecureScriptSummary) => {
    setEditingScript(script);
    updateForm.setFieldsValue({
      content: "",
      version: script.version,
      version_code: script.version_code
    });
  };

  const columns: ColumnsType<SecureScriptSummary> = [
    {
      title: "脚本",
      dataIndex: "name",
      key: "name",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text strong>{record.name}</Typography.Text>
          <Typography.Text type="secondary">
            {record.version} / 编号 {record.version_code}
          </Typography.Text>
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
      title: "哈希",
      dataIndex: "content_sha256",
      key: "content_sha256",
      render: (value: string) => shortId(value)
    },
    {
      title: "签名",
      dataIndex: "signature_kid",
      key: "signature_kid",
      render: (value: string) => value
    },
    {
      title: "特性",
      dataIndex: "required_features",
      key: "required_features",
      render: (features: unknown[]) =>
        features.length > 0
          ? features.map((feature) => <Tag key={String(feature)}>{String(feature)}</Tag>)
          : "-"
    },
    {
      title: "到期时间",
      dataIndex: "expires_at",
      key: "expires_at",
      render: (value?: string | null) => dateTime(value)
    },
    {
      title: "发布时间",
      dataIndex: "published_at",
      key: "published_at",
      render: (value?: string | null) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 220,
      render: (_, record) => (
        <Space>
          {canUpdate && record.status !== "deprecated" ? (
            <Button
              size="small"
              icon={<Edit3 size={14} />}
              onClick={() => openUpdate(record)}
            >
              内容
            </Button>
          ) : null}
          {canPublish && record.status === "draft" ? (
            <ConfirmActionButton
              title="发布脚本"
              description="发布后客户端可能拉取并执行该脚本，必须确认内容和签名无误。"
              buttonProps={{
                size: "small",
                icon: <Rocket size={14} />
              }}
              loading={publishMutation.isPending}
              onConfirm={() => publishMutation.mutate(record.id)}
            >
              发布
            </ConfirmActionButton>
          ) : null}
          {canDeprecate && record.status === "published" ? (
            <ConfirmActionButton
              title="废弃脚本"
              description="废弃后客户端不会继续把该脚本作为可用脚本拉取。"
              buttonProps={{
                size: "small",
                icon: <Archive size={14} />
              }}
              loading={deprecateMutation.isPending}
              onConfirm={() => deprecateMutation.mutate(record.id)}
            >
              废弃
            </ConfirmActionButton>
          ) : null}
        </Space>
      )
    }
  ];

  const submitCreate = (values: CreateScriptFormValues) => {
    createMutation.mutate(values);
  };

  const submitUpdate = (values: UpdateContentFormValues) => {
    updateMutation.mutate(values);
  };

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>脚本管理</Typography.Title>
          <Typography.Text type="secondary">安全脚本、加密内容和版本发布</Typography.Text>
        </div>
        <Space>
          <Select
            loading={appsQuery.isLoading}
            placeholder="应用"
            className="table-select"
            options={appOptions}
            value={appId}
            onChange={(value) => {
              setPage(1);
              setAppId(value);
            }}
          />
          <Select
            allowClear
            placeholder="状态"
            className="table-filter"
            value={status}
            options={[
              tOption("draft"),
              tOption("published"),
              tOption("deprecated")
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
          <Button icon={<RefreshCw size={16} />} onClick={() => scriptsQuery.refetch()} />
          {canCreate ? (
            <Button
              type="primary"
              icon={<Plus size={16} />}
              disabled={!appId}
              onClick={() => setCreateOpen(true)}
            >
              创建脚本
            </Button>
          ) : null}
        </Space>
      </div>
      {appsQuery.error || scriptsQuery.error ? (
        <Alert type="error" message={tMessage("secure_scripts_load_failed")} />
      ) : null}
      {createMutation.error ? (
        <Alert type="error" message={tMessage("secure_script_create_failed")} />
      ) : null}
      {updateMutation.error ? (
        <Alert type="error" message={tMessage("secure_script_content_update_failed")} />
      ) : null}
      {publishMutation.error || deprecateMutation.error ? (
        <Alert type="error" message={tMessage("secure_script_status_update_failed")} />
      ) : null}
      <Table
        rowKey="id"
        loading={appsQuery.isLoading || scriptsQuery.isLoading}
        columns={columns}
        dataSource={scriptsQuery.data?.items ?? []}
        pagination={false}
        locale={{ emptyText: "暂无数据" }}
      />
      <SimplePager
        page={page}
        pageSize={pageSize}
        itemCount={scriptsQuery.data?.items.length ?? 0}
        loading={scriptsQuery.isFetching}
        onChange={setPage}
      />

      <Modal
        title="创建脚本"
        open={createOpen}
        onCancel={() => setCreateOpen(false)}
        onOk={() => createForm.submit()}
        confirmLoading={createMutation.isPending}
        destroyOnClose
      >
        <Form<CreateScriptFormValues>
          form={createForm}
          layout="vertical"
          onFinish={submitCreate}
        >
          <Form.Item name="name" label="脚本名称" rules={[{ required: true, message: "请输入脚本名称" }]}>
            <Input />
          </Form.Item>
          <Form.Item name="version" label="版本号" rules={[{ required: true, message: "请输入版本号" }]}>
            <Input />
          </Form.Item>
          <Form.Item
            name="version_code"
            label="版本编号"
            rules={[{ required: true, message: "请输入版本编号" }]}
          >
            <InputNumber min={1} precision={0} className="form-number" />
          </Form.Item>
          <Form.Item name="required_features" label="所需功能">
            <Select mode="tags" tokenSeparators={[","]} />
          </Form.Item>
          <Form.Item name="expires_at" label="到期时间">
            <DatePicker showTime className="form-date" />
          </Form.Item>
          {canUpdate ? (
            <Form.Item name="content" label="脚本内容">
              <Input.TextArea rows={8} />
            </Form.Item>
          ) : null}
        </Form>
      </Modal>

      <Modal
        title="更新内容"
        open={Boolean(editingScript)}
        onCancel={() => {
          setEditingScript(null);
          updateForm.resetFields();
        }}
        onOk={() => updateForm.submit()}
        confirmLoading={updateMutation.isPending}
        destroyOnClose
      >
        <Form<UpdateContentFormValues>
          form={updateForm}
          layout="vertical"
          onFinish={submitUpdate}
        >
          <Form.Item name="version" label="版本号">
            <Input />
          </Form.Item>
          <Form.Item name="version_code" label="版本编号">
            <InputNumber min={1} precision={0} className="form-number" />
          </Form.Item>
          <Form.Item
            name="content"
            label="脚本内容"
            rules={[{ required: true, message: "请输入脚本内容" }]}
          >
            <Input.TextArea rows={10} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="脚本签名"
        open={Boolean(signedScript)}
        onCancel={() => setSignedScript(null)}
        onOk={() => setSignedScript(null)}
      >
        <Space direction="vertical" size={12} className="token-result">
          <Typography.Text type="secondary">脚本 ID（script_id）</Typography.Text>
          <Typography.Text copyable>{signedScript?.id}</Typography.Text>
          <Typography.Text type="secondary">内容哈希（content_sha256）</Typography.Text>
          <Typography.Text copyable>{signedScript?.content_sha256}</Typography.Text>
          <Typography.Text type="secondary">签名密钥 ID（signature_kid）</Typography.Text>
          <Typography.Text copyable>{signedScript?.signature_kid}</Typography.Text>
          <Typography.Text type="secondary">签名（signature）</Typography.Text>
          <Typography.Text copyable>{signedScript?.signature}</Typography.Text>
        </Space>
      </Modal>
    </section>
  );
}

function clean(value?: string): string | undefined {
  const trimmed = value?.trim();

  return trimmed ? trimmed : undefined;
}

function cleanNumber(value?: number | null): number | undefined {
  return typeof value === "number" ? value : undefined;
}

function requiredNumber(value?: number | null): number {
  return typeof value === "number" ? value : 0;
}

function cleanFeatures(values?: string[]): string[] | undefined {
  const features = values
    ?.map((value) => value.trim())
    .filter((value) => value.length > 0);

  return features && features.length > 0 ? features : undefined;
}

function encodeBase64(value: string): string {
  const bytes = new TextEncoder().encode(value);
  let binary = "";
  bytes.forEach((byte) => {
    binary += String.fromCharCode(byte);
  });

  return btoa(binary);
}
