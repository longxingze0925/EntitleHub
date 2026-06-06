import {
  Alert,
  Button,
  Form,
  Input,
  InputNumber,
  Modal,
  Popconfirm,
  Select,
  Space,
  Switch,
  Table,
  Tag,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMutation, useQuery } from "@tanstack/react-query";
import { Archive, Plus, RefreshCw, Rocket } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import {
  createRelease,
  deprecateRelease,
  listApplications,
  listReleases,
  publishRelease,
  uploadReleaseFile,
  type RegisterReleaseFileResult,
  type ReleaseSummary
} from "../../api/admin";
import { SimplePager } from "../../components/SimplePager";
import { StatusTag } from "../../components/StatusTag";
import { useAuthStore } from "../../stores/authStore";
import { dateTime, shortId } from "../../utils/format";
import { hasPermission } from "../../utils/permissions";

const pageSize = 20;

interface CreateReleaseFormValues {
  version: string;
  version_code?: number | null;
  changelog?: string;
  force_update?: boolean;
}

interface CreatedReleaseResult {
  file: RegisterReleaseFileResult;
  release: ReleaseSummary;
}

export function ReleasesPage() {
  const [appId, setAppId] = useState<string>();
  const [status, setStatus] = useState<string | undefined>();
  const [page, setPage] = useState(1);
  const [createOpen, setCreateOpen] = useState(false);
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [createdRelease, setCreatedRelease] =
    useState<CreatedReleaseResult | null>(null);
  const [form] = Form.useForm<CreateReleaseFormValues>();
  const permissions = useAuthStore((state) => state.permissions);
  const canCreate = hasPermission(permissions, "release:create");
  const canUpload = hasPermission(permissions, "release:upload");
  const canPublish = hasPermission(permissions, "release:publish");
  const canDeprecate = hasPermission(permissions, "release:deprecate");
  const appsQuery = useQuery({
    queryKey: ["admin", "apps", "release-selector"],
    queryFn: () => listApplications({})
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
    if (!appId && appOptions.length > 0) {
      setAppId(appOptions[0].value);
    }
  }, [appId, appOptions]);

  const releasesQuery = useQuery({
    queryKey: ["admin", "releases", appId, status, page],
    queryFn: () => listReleases({ appId: appId!, status, page, page_size: pageSize }),
    enabled: Boolean(appId)
  });
  const createMutation = useMutation({
    mutationFn: async (values: CreateReleaseFormValues) => {
      if (!appId) {
        throw new Error("application_required");
      }
      if (!selectedFile) {
        throw new Error("release_file_required");
      }

      const file = await uploadReleaseFile({
        appId,
        file: selectedFile
      });
      const release = await createRelease({
        appId,
        payload: {
          file_id: file.file_id,
          version: values.version.trim(),
          version_code: requiredNumber(values.version_code),
          changelog: clean(values.changelog),
          force_update: Boolean(values.force_update)
        }
      });

      return { file, release: release.release };
    },
    onSuccess: async (data) => {
      message.success("release_created");
      setCreatedRelease(data);
      setCreateOpen(false);
      setSelectedFile(null);
      form.resetFields();
      await releasesQuery.refetch();
    }
  });
  const publishMutation = useMutation({
    mutationFn: publishRelease,
    onSuccess: async () => {
      message.success("release_published");
      await releasesQuery.refetch();
    }
  });
  const deprecateMutation = useMutation({
    mutationFn: deprecateRelease,
    onSuccess: async () => {
      message.success("release_deprecated");
      await releasesQuery.refetch();
    }
  });

  const columns: ColumnsType<ReleaseSummary> = [
    {
      title: "版本",
      dataIndex: "version",
      key: "version",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text strong>{record.version}</Typography.Text>
          <Typography.Text type="secondary">code {record.version_code}</Typography.Text>
        </Space>
      )
    },
    {
      title: "文件",
      dataIndex: "file_id",
      key: "file_id",
      render: (value: string) => shortId(value)
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 110,
      render: (value) => <StatusTag value={value} />
    },
    {
      title: "强制",
      dataIndex: "force_update",
      key: "force_update",
      width: 90,
      render: (enabled: boolean) =>
        enabled ? <Tag color="red">force</Tag> : <Tag>normal</Tag>
    },
    {
      title: "发布时间",
      dataIndex: "published_at",
      key: "published_at",
      render: (value?: string | null) => dateTime(value)
    },
    {
      title: "更新时间",
      dataIndex: "updated_at",
      key: "updated_at",
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 170,
      render: (_, record) => (
        <Space>
          {canPublish && record.status === "draft" ? (
            <Popconfirm
              title="发布版本"
              onConfirm={() => publishMutation.mutate(record.id)}
            >
              <Button
                size="small"
                icon={<Rocket size={14} />}
                loading={publishMutation.isPending}
              >
                发布
              </Button>
            </Popconfirm>
          ) : null}
          {canDeprecate && record.status === "published" ? (
            <Popconfirm
              title="废弃版本"
              onConfirm={() => deprecateMutation.mutate(record.id)}
            >
              <Button
                size="small"
                icon={<Archive size={14} />}
                loading={deprecateMutation.isPending}
              >
                废弃
              </Button>
            </Popconfirm>
          ) : null}
        </Space>
      )
    }
  ];

  const submitCreate = (values: CreateReleaseFormValues) => {
    createMutation.mutate(values);
  };

  return (
    <section className="workspace-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>版本管理</Typography.Title>
          <Typography.Text type="secondary">版本发布、文件签名和下载分发</Typography.Text>
        </div>
        <Space>
          <Select
            loading={appsQuery.isLoading}
            placeholder="application"
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
            placeholder="status"
            className="table-filter"
            options={[
              { value: "draft", label: "draft" },
              { value: "published", label: "published" },
              { value: "deprecated", label: "deprecated" }
            ]}
            onChange={(value) => {
              setPage(1);
              setStatus(value);
            }}
          />
          <Button icon={<RefreshCw size={16} />} onClick={() => releasesQuery.refetch()} />
          {canUpload && canCreate ? (
            <Button
              type="primary"
              icon={<Plus size={16} />}
              disabled={!appId}
              onClick={() => setCreateOpen(true)}
            >
              创建版本
            </Button>
          ) : null}
        </Space>
      </div>
      {appsQuery.error || releasesQuery.error ? (
        <Alert type="error" message="releases_load_failed" />
      ) : null}
      {createMutation.error ? (
        <Alert type="error" message="release_create_failed" />
      ) : null}
      {publishMutation.error || deprecateMutation.error ? (
        <Alert type="error" message="release_status_update_failed" />
      ) : null}
      <Table
        rowKey="id"
        loading={appsQuery.isLoading || releasesQuery.isLoading}
        columns={columns}
        dataSource={releasesQuery.data?.items ?? []}
        pagination={false}
        locale={{ emptyText: "暂无数据" }}
      />
      <SimplePager
        page={page}
        pageSize={pageSize}
        itemCount={releasesQuery.data?.items.length ?? 0}
        loading={releasesQuery.isFetching}
        onChange={setPage}
      />

      <Modal
        title="创建版本"
        open={createOpen}
        onCancel={() => {
          setCreateOpen(false);
          setSelectedFile(null);
        }}
        onOk={() => form.submit()}
        confirmLoading={createMutation.isPending}
        destroyOnClose
      >
        <Form<CreateReleaseFormValues>
          form={form}
          layout="vertical"
          onFinish={submitCreate}
          initialValues={{ force_update: false }}
        >
          <Form.Item label="版本文件" required>
            <Input
              type="file"
              onChange={(event) => {
                setSelectedFile(event.target.files?.[0] ?? null);
              }}
            />
            {selectedFile ? (
              <Typography.Text type="secondary">
                {selectedFile.name} · {selectedFile.size} bytes
              </Typography.Text>
            ) : null}
          </Form.Item>
          <Form.Item
            name="version"
            label="版本号"
            rules={[{ required: true, message: "请输入版本号" }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="version_code"
            label="版本 Code"
            rules={[{ required: true, message: "请输入版本 Code" }]}
          >
            <InputNumber min={1} precision={0} className="form-number" />
          </Form.Item>
          <Form.Item name="force_update" label="强制更新" valuePropName="checked">
            <Switch />
          </Form.Item>
          <Form.Item name="changelog" label="更新说明">
            <Input.TextArea rows={3} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title="版本签名"
        open={Boolean(createdRelease)}
        onCancel={() => setCreatedRelease(null)}
        onOk={() => setCreatedRelease(null)}
      >
        <Space direction="vertical" size={12} className="token-result">
          <Typography.Text type="secondary">release_id</Typography.Text>
          <Typography.Text copyable>{createdRelease?.release.id}</Typography.Text>
          <Typography.Text type="secondary">file_id</Typography.Text>
          <Typography.Text copyable>{createdRelease?.file.file_id}</Typography.Text>
          <Typography.Text type="secondary">signature_kid</Typography.Text>
          <Typography.Text copyable>{createdRelease?.file.signature_kid}</Typography.Text>
          <Typography.Text type="secondary">signature</Typography.Text>
          <Typography.Text copyable>{createdRelease?.file.signature}</Typography.Text>
        </Space>
      </Modal>
    </section>
  );
}

function clean(value?: string): string | undefined {
  const trimmed = value?.trim();

  return trimmed ? trimmed : undefined;
}

function requiredNumber(value?: number | null): number {
  return typeof value === "number" ? value : 0;
}
