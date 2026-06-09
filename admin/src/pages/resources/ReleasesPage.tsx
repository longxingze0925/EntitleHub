import {
  Alert,
  Button,
  Form,
  Input,
  InputNumber,
  Modal,
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
import { Archive, Edit3, KeyRound, Plus, RefreshCw, Rocket, Trash2 } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import {
  createRelease,
  deleteRelease,
  deprecateRelease,
  getRelease,
  listApplications,
  listReleases,
  publishRelease,
  updateRelease,
  uploadReleaseFile,
  type ReleaseDetailResult,
  type RegisterReleaseFileResult,
  type ReleaseSummary
} from "../../api/admin";
import { ConfirmActionButton } from "../../components/ConfirmActionButton";
import { HistoryToggle } from "../../components/HistoryToggle";
import { SimplePager } from "../../components/SimplePager";
import { StatusTag } from "../../components/StatusTag";
import { useAuthStore } from "../../stores/authStore";
import { dateTime, shortId } from "../../utils/format";
import { tMessage, tOption, tStatus } from "../../utils/i18n";
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
  const [includeHistory, setIncludeHistory] = useState(false);
  const [page, setPage] = useState(1);
  const [createOpen, setCreateOpen] = useState(false);
  const [editingRelease, setEditingRelease] = useState<ReleaseSummary | null>(null);
  const [selectedFile, setSelectedFile] = useState<File | null>(null);
  const [createdRelease, setCreatedRelease] =
    useState<CreatedReleaseResult | null>(null);
  const [releaseDetail, setReleaseDetail] = useState<ReleaseDetailResult | null>(null);
  const [form] = Form.useForm<CreateReleaseFormValues>();
  const [editForm] = Form.useForm<CreateReleaseFormValues>();
  const permissions = useAuthStore((state) => state.permissions);
  const canCreate = hasPermission(permissions, "release:create");
  const canUpload = hasPermission(permissions, "release:upload");
  const canUpdate = hasPermission(permissions, "release:update");
  const canPublish = hasPermission(permissions, "release:publish");
  const canDeprecate = hasPermission(permissions, "release:deprecate");
  const canDelete = hasPermission(permissions, "release:delete");
  const appsQuery = useQuery({
    queryKey: ["admin", "apps", "release-selector", includeHistory],
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

  const releasesQuery = useQuery({
    queryKey: ["admin", "releases", appId, status, includeHistory, page],
    queryFn: () =>
      listReleases({
        appId: appId!,
        status,
        include_history: includeHistory,
        page,
        page_size: pageSize
      }),
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
      message.success(tMessage("release_created"));
      setCreatedRelease(data);
      setCreateOpen(false);
      setSelectedFile(null);
      form.resetFields();
      await releasesQuery.refetch();
    }
  });
  const detailMutation = useMutation({
    mutationFn: getRelease,
    onSuccess: (data) => {
      setReleaseDetail(data);
    }
  });
  const updateMutation = useMutation({
    mutationFn: async (values: CreateReleaseFormValues) => {
      if (!editingRelease) {
        throw new Error("release_required");
      }

      const updated = await updateRelease({
        id: editingRelease.id,
        payload: {
          version: values.version.trim(),
          version_code: requiredNumber(values.version_code),
          changelog: clean(values.changelog),
          force_update: Boolean(values.force_update)
        }
      });

      return updated.release;
    },
    onSuccess: async () => {
      message.success(tMessage("release_updated"));
      setEditingRelease(null);
      editForm.resetFields();
      await releasesQuery.refetch();
    }
  });
  const publishMutation = useMutation({
    mutationFn: publishRelease,
    onSuccess: async () => {
      message.success(tMessage("release_published"));
      await releasesQuery.refetch();
    }
  });
  const deprecateMutation = useMutation({
    mutationFn: deprecateRelease,
    onSuccess: async () => {
      message.success(tMessage("release_deprecated"));
      await releasesQuery.refetch();
    }
  });
  const deleteMutation = useMutation({
    mutationFn: deleteRelease,
    onSuccess: async () => {
      message.success(tMessage("release_deleted"));
      await releasesQuery.refetch();
    }
  });

  const openEdit = (release: ReleaseSummary) => {
    setEditingRelease(release);
    editForm.setFieldsValue({
      version: release.version,
      version_code: release.version_code,
      changelog: release.changelog ?? undefined,
      force_update: release.force_update
    });
  };

  const columns: ColumnsType<ReleaseSummary> = [
    {
      title: "版本",
      dataIndex: "version",
      key: "version",
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text strong>{record.version}</Typography.Text>
          <Typography.Text type="secondary">编号 {record.version_code}</Typography.Text>
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
        enabled ? <Tag color="red">{tStatus("force")}</Tag> : <Tag>{tStatus("normal")}</Tag>
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
      width: 320,
      render: (_, record) => (
        <Space className="table-actions-nowrap">
          <Button
            size="small"
            icon={<KeyRound size={14} />}
            loading={detailMutation.isPending}
            onClick={() => detailMutation.mutate(record.id)}
          >
            签名
          </Button>
          {canUpdate && record.status === "draft" ? (
            <Button
              size="small"
              icon={<Edit3 size={14} />}
              onClick={() => openEdit(record)}
            >
              编辑
            </Button>
          ) : null}
          {canPublish && record.status === "draft" ? (
            <ConfirmActionButton
              title="发布版本"
              description="发布后客户端可能检查到该版本并开始下载更新。"
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
          {canDelete && record.status === "draft" ? (
            <ConfirmActionButton
              title="删除草稿"
              description="只会删除当前草稿记录，不会删除已经上传的版本文件。删除后可以重新创建同版本。"
              buttonProps={{
                size: "small",
                danger: true,
                icon: <Trash2 size={14} />
              }}
              loading={deleteMutation.isPending}
              onConfirm={() => deleteMutation.mutate(record.id)}
            >
              删除
            </ConfirmActionButton>
          ) : null}
          {canDeprecate && record.status === "published" ? (
            <ConfirmActionButton
              title="废弃版本"
              description="废弃后该版本不会继续作为可用更新分发。"
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
        <Alert type="error" message={tMessage("releases_load_failed")} />
      ) : null}
      {createMutation.error ? (
        <Alert type="error" message={tMessage("release_create_failed")} />
      ) : null}
      {detailMutation.error ? (
        <Alert type="error" message={tMessage("release_detail_failed")} />
      ) : null}
      {updateMutation.error ? (
        <Alert type="error" message={tMessage("release_update_failed")} />
      ) : null}
      {deleteMutation.error ? (
        <Alert type="error" message={tMessage("release_delete_failed")} />
      ) : null}
      {publishMutation.error || deprecateMutation.error ? (
        <Alert type="error" message={tMessage("release_status_update_failed")} />
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
                {selectedFile.name} · {selectedFile.size} 字节
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
            label="版本编号"
            rules={[{ required: true, message: "请输入版本编号" }]}
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
        title="编辑版本"
        open={Boolean(editingRelease)}
        onCancel={() => {
          setEditingRelease(null);
          editForm.resetFields();
        }}
        onOk={() => editForm.submit()}
        confirmLoading={updateMutation.isPending}
        destroyOnClose
      >
        <Form<CreateReleaseFormValues>
          form={editForm}
          layout="vertical"
          onFinish={(values) => updateMutation.mutate(values)}
        >
          <Form.Item
            name="version"
            label="版本号"
            rules={[{ required: true, message: "请输入版本号" }]}
          >
            <Input />
          </Form.Item>
          <Form.Item
            name="version_code"
            label="版本编号"
            rules={[{ required: true, message: "请输入版本编号" }]}
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
          <Typography.Text type="secondary">版本 ID（release_id）</Typography.Text>
          <Typography.Text copyable>{createdRelease?.release.id}</Typography.Text>
          <Typography.Text type="secondary">文件 ID（file_id）</Typography.Text>
          <Typography.Text copyable>{createdRelease?.file.file_id}</Typography.Text>
          <Typography.Text type="secondary">签名密钥 ID（signature_kid）</Typography.Text>
          <Typography.Text copyable>{createdRelease?.file.signature_kid}</Typography.Text>
          <Typography.Text type="secondary">签名（signature）</Typography.Text>
          <Typography.Text copyable>{createdRelease?.file.signature}</Typography.Text>
        </Space>
      </Modal>

      <Modal
        title="签名信息"
        open={Boolean(releaseDetail)}
        onCancel={() => setReleaseDetail(null)}
        onOk={() => setReleaseDetail(null)}
        width={720}
      >
        <Space direction="vertical" size={12} className="token-result">
          <Typography.Text type="secondary">版本 ID（release_id）</Typography.Text>
          <Typography.Text copyable>{releaseDetail?.release.id}</Typography.Text>
          <Typography.Text type="secondary">文件 ID（file_id）</Typography.Text>
          <Typography.Text copyable>{releaseDetail?.file.id}</Typography.Text>
          <Typography.Text type="secondary">文件名（file_name）</Typography.Text>
          <Typography.Text copyable>{releaseDetail?.file.file_name}</Typography.Text>
          <Typography.Text type="secondary">文件大小（file_size）</Typography.Text>
          <Typography.Text copyable>{releaseDetail?.file.file_size}</Typography.Text>
          <Typography.Text type="secondary">文件哈希（sha256）</Typography.Text>
          <Typography.Text copyable>{releaseDetail?.file.sha256}</Typography.Text>
          <Typography.Text type="secondary">文件签名密钥 ID（signature_kid）</Typography.Text>
          <Typography.Text copyable>{releaseDetail?.file.signature_kid}</Typography.Text>
          <Typography.Text type="secondary">文件签名算法（signature_alg）</Typography.Text>
          <Typography.Text copyable>{releaseDetail?.file.signature_alg}</Typography.Text>
          <Typography.Text type="secondary">文件签名（signature）</Typography.Text>
          <Typography.Text copyable>{releaseDetail?.file.signature}</Typography.Text>
          <Typography.Text type="secondary">版本签名密钥 ID（signature_kid）</Typography.Text>
          <Typography.Text copyable>
            {releaseDetail?.release.signature_kid ?? "发布后生成"}
          </Typography.Text>
          <Typography.Text type="secondary">版本签名算法（signature_alg）</Typography.Text>
          <Typography.Text copyable>
            {releaseDetail?.release.signature_alg ?? "发布后生成"}
          </Typography.Text>
          <Typography.Text type="secondary">版本签名（signature）</Typography.Text>
          <Typography.Text copyable>
            {releaseDetail?.release.signature ?? "发布后生成"}
          </Typography.Text>
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
