import {
  Alert,
  AutoComplete,
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
  Tooltip,
  Typography,
  message
} from "antd";
import type { ColumnsType } from "antd/es/table";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Ban,
  Coins,
  Eye,
  History,
  KeyRound,
  Pencil,
  Plus,
  RefreshCw,
  RotateCw,
  Trash2,
  Undo2
} from "lucide-react";
import { useState } from "react";
import { useLocation } from "react-router-dom";

import {
  adjustAiWallet,
  createAiApiKey,
  createAiModel,
  createAiProvider,
  deleteAiAsset,
  failReleaseAiGenerationJob,
  getAiGenerationJob,
  listAiApiKeys,
  listAiAssets,
  listAiGenerationJobs,
  listAiModels,
  listAiProviders,
  listAiUsageRecords,
  listAiWalletLedger,
  listAiWallets,
  refundAiGenerationJob,
  retryAiGenerationJobCache,
  retryAiGenerationJobPoll,
  revokeAiApiKey,
  updateAiApiKey,
  updateAiModel,
  updateAiProvider,
  updateAiWalletAccess,
  updateAiWalletQuota,
  type AiAsset,
  type AiAssetStatus,
  type AiAssetType,
  type AiGenerationJob,
  type AiGenerationJobDetail,
  type AiGenerationJobStatus,
  type AiGenerationJobType,
  type AiApiKey,
  type AiModel,
  type AiModelBillingMode,
  type AiModelModality,
  type AiProvider,
  type AiProviderKind,
  type AiUsageRecord,
  type AiWallet,
  type AiWalletLedgerEntry
} from "../../api/admin";
import { ConfirmActionButton } from "../../components/ConfirmActionButton";
import { HistoryToggle } from "../../components/HistoryToggle";
import { useAuthStore } from "../../stores/authStore";
import { dateTime } from "../../utils/format";
import { tApiError } from "../../utils/i18n";
import { hasPermission } from "../../utils/permissions";

const AI_TABLE_SCROLL = { x: "max-content" } as const;

interface ProviderFormValues {
  name: string;
  kind: AiProviderKind;
  base_url: string;
  enabled: boolean;
  api_key?: string;
  config_json: string;
}

interface ModelFormValues {
  code: string;
  name: string;
  modality: AiModelModality;
  provider_id?: string;
  provider_model?: string;
  enabled: boolean;
  currency: string;
  billing_mode: AiModelBillingMode;
  input_1k_price: number;
  output_1k_price: number;
  request_price: number;
  image_price: number;
  second_price: number;
  minute_price: number;
  daily_spend_limit?: number | null;
  ratios?: string[];
  resolutions?: string[];
  durations?: Array<string | number>;
  default_duration_seconds?: number | null;
  image_counts?: Array<string | number>;
  max_images?: number | null;
  pricing_config_json: string;
  metadata_json: string;
}

interface ProviderModelTemplate {
  provider_model: string;
  name?: string;
  modality: AiModelModality;
  billing_mode?: AiModelBillingMode;
  pricing_config?: Record<string, unknown>;
  metadata?: Record<string, unknown>;
}

interface WalletAdjustFormValues {
  direction: "credit" | "debit";
  amount: number;
  reason: string;
}

interface ApiKeyFormValues {
  customer_id: string;
  name: string;
  daily_spend_limit?: number | null;
}

interface ApiKeyEditFormValues {
  name: string;
  daily_spend_limit?: number | null;
}

interface WalletQuotaFormValues {
  daily_spend_limit?: number | null;
}

const providerKindOptions: Array<{ label: string; value: AiProviderKind }> = [
  { label: "OpenAI 兼容", value: "openai_compatible" },
  { label: "自定义 HTTP", value: "custom_http" },
  { label: "Claude", value: "claude" },
  { label: "Gemini", value: "gemini" },
  { label: "DeepSeek", value: "deepseek" },
  { label: "图片平台", value: "image" },
  { label: "视频平台", value: "video" },
  { label: "速创平台", value: "wuyin_keji" }
];

const modalityOptions: Array<{ label: string; value: AiModelModality }> = [
  { label: "文本", value: "text" },
  { label: "图片", value: "image" },
  { label: "视频", value: "video" },
  { label: "音频", value: "audio" },
  { label: "向量", value: "embedding" },
  { label: "多模态", value: "multimodal" }
];

const billingModeLabels: Record<AiModelBillingMode, string> = {
  token: "输入/输出 token",
  per_image: "按张图片",
  video_per_second: "视频按秒",
  video_per_request: "视频按次",
  audio_per_second: "音频按秒",
  audio_per_minute: "音频按分钟",
  audio_per_request: "音频按次"
};

const billingModeOptionsByModality: Record<
  AiModelModality,
  Array<{ label: string; value: AiModelBillingMode }>
> = {
  text: [{ label: billingModeLabels.token, value: "token" }],
  embedding: [{ label: billingModeLabels.token, value: "token" }],
  image: [{ label: billingModeLabels.per_image, value: "per_image" }],
  video: [
    { label: billingModeLabels.video_per_second, value: "video_per_second" },
    { label: billingModeLabels.video_per_request, value: "video_per_request" }
  ],
  audio: [
    { label: billingModeLabels.audio_per_second, value: "audio_per_second" },
    { label: billingModeLabels.audio_per_minute, value: "audio_per_minute" },
    { label: billingModeLabels.audio_per_request, value: "audio_per_request" }
  ],
  multimodal: [
    { label: billingModeLabels.token, value: "token" },
    { label: billingModeLabels.per_image, value: "per_image" }
  ]
};

const defaultJson = "{\n}";
const defaultImageRatios = ["1:1", "3:4", "4:3", "9:16", "16:9"];
const defaultVideoRatios = ["16:9", "9:16", "1:1"];
const defaultImageResolutions = ["1024x1024", "768x1024", "1024x768"];
const defaultVideoResolutions = ["720p", "1080p", "1280x720", "1920x1080"];
const defaultVideoDurations = [5, 8, 10];
const defaultImageCounts = [1, 2, 4];

const wuyinKejiModelTemplates: ProviderModelTemplate[] = [
  {
    provider_model: "GPT-Image-2",
    name: "GPT-Image-2 图片生成",
    modality: "image",
    billing_mode: "per_image",
    pricing_config: {
      submit_path: "/api/async/image_gpt",
      capabilities: {
        ratios: ["1:1", "3:2", "2:3", "16:9", "9:16"],
        image_counts: [1],
        max_images: 1
      }
    },
    metadata: {
      provider_doc_url: "https://api.wuyinkeji.com/doc/53"
    }
  },
  {
    provider_model: "NanoBanana2",
    name: "NanoBanana2 图片生成",
    modality: "image",
    billing_mode: "per_image",
    pricing_config: {
      submit_path: "/api/async/image_nanoBanana2",
      capabilities: {
        ratios: ["1:1", "3:4", "4:3", "9:16", "16:9"],
        resolutions: ["1K", "2K", "4K"],
        image_counts: [1],
        max_images: 1
      }
    },
    metadata: {
      provider_doc_url: "https://api.wuyinkeji.com/doc/65"
    }
  },
  {
    provider_model: "google_omni",
    name: "Google Omni 视频生成",
    modality: "video",
    billing_mode: "video_per_second",
    pricing_config: {
      submit_path: "/api/async/video_google_omni",
      capabilities: {
        resolutions: ["横版", "竖版"],
        durations: [10],
        default_duration_seconds: 10
      }
    },
    metadata: {
      provider_doc_url: "https://api.wuyinkeji.com/doc/72"
    }
  },
  {
    provider_model: "grok_imagine",
    name: "Grok Imagine 视频生成",
    modality: "video",
    billing_mode: "video_per_second",
    pricing_config: {
      submit_path: "/api/async/video_grok_imagine",
      capabilities: {
        ratios: ["16:9", "9:16"],
        durations: [6, 8, 10, 12, 15],
        default_duration_seconds: 8
      }
    },
    metadata: {
      provider_doc_url: "https://api.wuyinkeji.com/doc/62"
    }
  }
];

type AiBillingSection = "providers" | "models" | "wallets" | "jobs" | "usage" | "assets";

const sectionTitles: Record<AiBillingSection, { title: string; subtitle: string }> = {
  providers: {
    title: "渠道管理",
    subtitle: "三方 AI 接口渠道、接口地址和密钥配置"
  },
  models: {
    title: "模型商品",
    subtitle: "模型代码、三方模型名、可用参数和不同类型的计费规则"
  },
  wallets: {
    title: "客户余额",
    subtitle: "客户 AI 余额、每日限额和权限冻结"
  },
  jobs: {
    title: "生成任务",
    subtitle: "异步图片和视频生成任务、三方状态、素材缓存和结算状态"
  },
  usage: {
    title: "调用日志",
    subtitle: "AI 请求、扣费、退款和失败状态"
  },
  assets: {
    title: "缓存素材",
    subtitle: "生成图片、视频、音频等素材缓存"
  }
};

export function AiBillingPage() {
  const location = useLocation();
  const [providerForm] = Form.useForm<ProviderFormValues>();
  const [modelForm] = Form.useForm<ModelFormValues>();
  const [walletForm] = Form.useForm<WalletAdjustFormValues>();
  const [walletQuotaForm] = Form.useForm<WalletQuotaFormValues>();
  const [apiKeyForm] = Form.useForm<ApiKeyFormValues>();
  const [apiKeyEditForm] = Form.useForm<ApiKeyEditFormValues>();
  const [providerModalOpen, setProviderModalOpen] = useState(false);
  const [modelModalOpen, setModelModalOpen] = useState(false);
  const [walletModalOpen, setWalletModalOpen] = useState(false);
  const [walletQuotaModalOpen, setWalletQuotaModalOpen] = useState(false);
  const [ledgerModalOpen, setLedgerModalOpen] = useState(false);
  const [apiKeyModalOpen, setApiKeyModalOpen] = useState(false);
  const [apiKeyEditModalOpen, setApiKeyEditModalOpen] = useState(false);
  const [generatedApiKey, setGeneratedApiKey] = useState<string | null>(null);
  const [editingProvider, setEditingProvider] = useState<AiProvider | null>(null);
  const [editingModel, setEditingModel] = useState<AiModel | null>(null);
  const [editingApiKey, setEditingApiKey] = useState<AiApiKey | null>(null);
  const [selectedWallet, setSelectedWallet] = useState<AiWallet | null>(null);
  const [selectedGenerationJob, setSelectedGenerationJob] = useState<AiGenerationJob | null>(null);
  const [generationJobDetailOpen, setGenerationJobDetailOpen] = useState(false);
  const [includeHistory, setIncludeHistory] = useState(false);
  const queryClient = useQueryClient();
  const permissions = useAuthStore((state) => state.permissions);
  const canUpdateProvider = hasPermission(permissions, "ai:provider:update");
  const canUpdateModel = hasPermission(permissions, "ai:model:update");
  const canUpdateWallet = hasPermission(permissions, "ai:wallet:update");
  const canUpdateApiKey = hasPermission(permissions, "ai:api_key:update");
  const canDeleteAsset = hasPermission(permissions, "ai:asset:delete");
  const canUpdateJob = hasPermission(permissions, "ai:job:update");
  // API Key is reserved for future OpenAPI/server integrations; normal clients use session auth.
  const showOpenApiKeyManagement = false;
  const currentSection = aiBillingSectionFromPath(location.pathname);
  const heading = sectionTitles[currentSection];
  const selectedModelModality = (Form.useWatch("modality", modelForm) ??
    "text") as AiModelModality;
  const selectedProviderId = Form.useWatch("provider_id", modelForm);
  const selectedBillingMode =
    ((Form.useWatch("billing_mode", modelForm) ??
      defaultBillingModeForModality(selectedModelModality)) as AiModelBillingMode);
  const billingModeOptions = billingModeOptionsByModality[selectedModelModality];

  const providersQuery = useQuery({
    queryKey: ["admin", "ai-providers", includeHistory],
    queryFn: () => listAiProviders({ include_history: includeHistory })
  });
  const selectedProvider = (providersQuery.data?.items ?? []).find(
    (provider) => provider.id === selectedProviderId
  );
  const selectedProviderTemplates = templatesForProviderAndModality(
    selectedProvider,
    selectedModelModality
  );
  const providerModelOptions = selectedProviderTemplates.map((template) => ({
    value: template.provider_model,
    label: template.name ? `${template.name}（${template.provider_model}）` : template.provider_model
  }));

  const modelsQuery = useQuery({
    queryKey: ["admin", "ai-models", includeHistory],
    queryFn: () => listAiModels({ include_history: includeHistory })
  });

  const walletsQuery = useQuery({
    queryKey: ["admin", "ai-wallets", includeHistory],
    queryFn: () => listAiWallets({ include_history: includeHistory })
  });

  const apiKeysQuery = useQuery({
    queryKey: ["admin", "ai-api-keys", includeHistory],
    queryFn: () => listAiApiKeys({ include_history: includeHistory }),
    enabled: showOpenApiKeyManagement
  });

  const usageRecordsQuery = useQuery({
    queryKey: ["admin", "ai-usage-records"],
    queryFn: () => listAiUsageRecords({ page: 1, page_size: 50 })
  });

  const generationJobsQuery = useQuery({
    queryKey: ["admin", "ai-generation-jobs"],
    queryFn: () => listAiGenerationJobs({ page: 1, page_size: 50 })
  });

  const generationJobDetailQuery = useQuery({
    queryKey: ["admin", "ai-generation-job", selectedGenerationJob?.id],
    queryFn: () => getAiGenerationJob(selectedGenerationJob?.id ?? ""),
    enabled: generationJobDetailOpen && Boolean(selectedGenerationJob)
  });

  const assetsQuery = useQuery({
    queryKey: ["admin", "ai-assets"],
    queryFn: () => listAiAssets({ page: 1, page_size: 50 })
  });

  const ledgerQuery = useQuery({
    queryKey: ["admin", "ai-wallet-ledger", selectedWallet?.customer_id],
    queryFn: () =>
      listAiWalletLedger({
        customerId: selectedWallet?.customer_id ?? "",
        page: 1,
        page_size: 20
      }),
    enabled: ledgerModalOpen && Boolean(selectedWallet)
  });

  const invalidateGenerationJobData = (jobId?: string) => {
    void queryClient.invalidateQueries({ queryKey: ["admin", "ai-generation-jobs"] });
    void queryClient.invalidateQueries({ queryKey: ["admin", "ai-usage-records"] });
    void queryClient.invalidateQueries({ queryKey: ["admin", "ai-wallets"] });
    void queryClient.invalidateQueries({ queryKey: ["admin", "ai-wallet-ledger"] });
    void queryClient.invalidateQueries({ queryKey: ["admin", "ai-assets"] });
    if (jobId) {
      void queryClient.invalidateQueries({ queryKey: ["admin", "ai-generation-job", jobId] });
    }
  };

  const providerMutation = useMutation({
    mutationFn: (values: ProviderFormValues) => {
      const payload = buildProviderPayload(values, Boolean(editingProvider));
      if (editingProvider) {
        return updateAiProvider({
          id: editingProvider.id,
          payload
        });
      }

      return createAiProvider({
        name: payload.name ?? values.name.trim(),
        kind: values.kind,
        base_url: payload.base_url ?? values.base_url.trim(),
        enabled: payload.enabled,
        config: payload.config ?? {},
        secret: payload.secret
      });
    },
    onSuccess: () => {
      message.success("AI 渠道已保存");
      setProviderModalOpen(false);
      setEditingProvider(null);
      providerForm.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin", "ai-providers"] });
    }
  });

  const modelMutation = useMutation({
    mutationFn: (values: ModelFormValues) => {
      const payload = buildModelPayload(values);
      if (editingModel) {
        return updateAiModel({
          id: editingModel.id,
          payload: {
            ...payload,
            provider_id: values.provider_id || null,
            provider_model: values.provider_model?.trim() || null
          }
        });
      }

      return createAiModel({
        code: values.code.trim(),
        ...payload,
        provider_id: values.provider_id || undefined,
        provider_model: values.provider_model?.trim() || undefined
      });
    },
    onSuccess: () => {
      message.success("AI 模型商品已保存");
      setModelModalOpen(false);
      setEditingModel(null);
      modelForm.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin", "ai-models"] });
    }
  });

  const walletMutation = useMutation({
    mutationFn: (values: WalletAdjustFormValues) => {
      if (!selectedWallet) {
        throw new Error("wallet not selected");
      }
      const amountMinor = moneyToMinor(values.amount) * (values.direction === "debit" ? -1 : 1);

      return adjustAiWallet({
        customerId: selectedWallet.customer_id,
        payload: {
          amount_minor: amountMinor,
          reason: values.reason.trim()
        }
      });
    },
    onSuccess: () => {
      message.success("AI 钱包余额已更新");
      setWalletModalOpen(false);
      walletForm.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin", "ai-wallets"] });
      queryClient.invalidateQueries({ queryKey: ["admin", "ai-wallet-ledger"] });
    }
  });

  const walletQuotaMutation = useMutation({
    mutationFn: (values: WalletQuotaFormValues) => {
      if (!selectedWallet) {
        throw new Error("wallet not selected");
      }

      return updateAiWalletQuota({
        customerId: selectedWallet.customer_id,
        payload: {
          daily_spend_limit_minor:
            values.daily_spend_limit == null ? null : moneyToMinor(values.daily_spend_limit)
        }
      });
    },
    onSuccess: () => {
      message.success("AI 钱包限额已更新");
      setWalletQuotaModalOpen(false);
      walletQuotaForm.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin", "ai-wallets"] });
    }
  });

  const walletAccessMutation = useMutation({
    mutationFn: ({ wallet, ai_enabled }: { wallet: AiWallet; ai_enabled: boolean }) =>
      updateAiWalletAccess({
        customerId: wallet.customer_id,
        payload: { ai_enabled }
      }),
    onSuccess: (_, variables) => {
      message.success(variables.ai_enabled ? "AI 权限已恢复" : "AI 权限已冻结");
      queryClient.invalidateQueries({ queryKey: ["admin", "ai-wallets"] });
    }
  });

  const retryJobPollMutation = useMutation({
    mutationFn: (job: AiGenerationJob) =>
      retryAiGenerationJobPoll(job.id, "后台手动重新查询第三方任务"),
    onSuccess: (_, job) => {
      message.success("已重新加入查询队列");
      invalidateGenerationJobData(job.id);
    },
    onError: (error) => {
      message.error(tApiError(error));
    }
  });

  const retryJobCacheMutation = useMutation({
    mutationFn: (job: AiGenerationJob) =>
      retryAiGenerationJobCache(job.id, "后台手动重新缓存生成素材"),
    onSuccess: (_, job) => {
      message.success("已重新执行素材缓存");
      invalidateGenerationJobData(job.id);
    },
    onError: (error) => {
      message.error(tApiError(error));
    }
  });

  const failReleaseJobMutation = useMutation({
    mutationFn: (job: AiGenerationJob) =>
      failReleaseAiGenerationJob(job.id, "后台手动标记失败并释放预扣"),
    onSuccess: (_, job) => {
      message.success("任务已标记失败，预扣已释放");
      invalidateGenerationJobData(job.id);
    },
    onError: (error) => {
      message.error(tApiError(error));
    }
  });

  const refundJobMutation = useMutation({
    mutationFn: (job: AiGenerationJob) => refundAiGenerationJob(job.id, "后台人工退款"),
    onSuccess: (_, job) => {
      message.success("任务已人工退款");
      invalidateGenerationJobData(job.id);
    },
    onError: (error) => {
      message.error(tApiError(error));
    }
  });

  const apiKeyMutation = useMutation({
    mutationFn: (values: ApiKeyFormValues) =>
      createAiApiKey({
        customerId: values.customer_id,
        payload: {
          name: values.name.trim(),
          daily_spend_limit_minor:
            values.daily_spend_limit == null ? null : moneyToMinor(values.daily_spend_limit)
        }
      }),
    onSuccess: (result) => {
      message.success("AI API Key 已生成");
      setGeneratedApiKey(result.plain_key);
      queryClient.invalidateQueries({ queryKey: ["admin", "ai-api-keys"] });
    }
  });

  const updateApiKeyMutation = useMutation({
    mutationFn: (values: ApiKeyEditFormValues) => {
      if (!editingApiKey) {
        throw new Error("api key not selected");
      }

      return updateAiApiKey({
        id: editingApiKey.id,
        payload: {
          name: values.name.trim(),
          daily_spend_limit_minor:
            values.daily_spend_limit == null ? null : moneyToMinor(values.daily_spend_limit)
        }
      });
    },
    onSuccess: () => {
      message.success("AI API Key 已更新");
      setApiKeyEditModalOpen(false);
      setEditingApiKey(null);
      apiKeyEditForm.resetFields();
      queryClient.invalidateQueries({ queryKey: ["admin", "ai-api-keys"] });
    }
  });

  const revokeApiKeyMutation = useMutation({
    mutationFn: (id: string) => revokeAiApiKey(id),
    onSuccess: () => {
      message.success("AI API Key 已吊销");
      queryClient.invalidateQueries({ queryKey: ["admin", "ai-api-keys"] });
    }
  });

  const deleteAssetMutation = useMutation({
    mutationFn: (id: string) => deleteAiAsset(id),
    onSuccess: () => {
      message.success("AI 缓存素材已删除");
      queryClient.invalidateQueries({ queryKey: ["admin", "ai-assets"] });
    }
  });

  const openCreateProvider = () => {
    const kind: AiProviderKind = "openai_compatible";
    setEditingProvider(null);
    providerForm.setFieldsValue({
      name: "",
      kind,
      base_url: "",
      enabled: true,
      config_json: stringifyJson(defaultProviderConfigForKind(kind))
    });
    setProviderModalOpen(true);
  };

  const openEditProvider = (provider: AiProvider) => {
    setEditingProvider(provider);
    providerForm.setFieldsValue({
      name: provider.name,
      kind: provider.kind,
      base_url: provider.base_url,
      enabled: provider.enabled,
      config_json: stringifyJson(provider.config)
    });
    setProviderModalOpen(true);
  };

  const openCreateModel = () => {
    setEditingModel(null);
    modelForm.setFieldsValue({
      code: "",
      name: "",
      modality: "text",
      enabled: true,
      currency: "CNY",
      billing_mode: "token",
      input_1k_price: 0,
      output_1k_price: 0,
      request_price: 0,
      image_price: 0,
      second_price: 0,
      minute_price: 0,
      daily_spend_limit: null,
      ratios: [],
      resolutions: [],
      durations: [],
      default_duration_seconds: null,
      image_counts: [],
      max_images: null,
      pricing_config_json: defaultJson,
      metadata_json: defaultJson
    });
    setModelModalOpen(true);
  };

  const applyProviderModelTemplate = (
    template: ProviderModelTemplate | undefined,
    options: { applyName?: boolean } = {}
  ) => {
    if (!template) {
      return;
    }
    modelForm.setFieldsValue(modelFormValuesFromTemplate(template, options));
  };

  const clearProviderModelTemplate = (modality: AiModelModality) => {
    modelForm.setFieldsValue({
      provider_model: undefined,
      pricing_config_json: defaultJson,
      metadata_json: defaultJson,
      ...defaultCapabilitiesForModality(modality)
    });
  };

  const openEditModel = (model: AiModel) => {
    setEditingModel(model);
    const capabilities = modelCapabilities(model.pricing_config);
    modelForm.setFieldsValue({
      code: model.code,
      name: model.name,
      modality: model.modality,
      provider_id: model.provider_id ?? undefined,
      provider_model: model.provider_model ?? undefined,
      enabled: model.enabled,
      currency: model.currency,
      billing_mode: model.billing_mode ?? defaultBillingModeForModality(model.modality),
      input_1k_price: minorToMoneyNumber(model.input_1k_price_minor),
      output_1k_price: minorToMoneyNumber(model.output_1k_price_minor),
      request_price: minorToMoneyNumber(model.request_price_minor),
      image_price: minorToMoneyNumber(model.image_price_minor),
      second_price: minorToMoneyNumber(model.second_price_minor),
      minute_price: minorToMoneyNumber(model.minute_price_minor ?? 0),
      daily_spend_limit:
        model.daily_spend_limit_minor == null
          ? null
          : minorToMoneyNumber(model.daily_spend_limit_minor),
      ratios: capabilities.ratios,
      resolutions: capabilities.resolutions,
      durations: capabilities.durations.map(String),
      default_duration_seconds: capabilities.default_duration_seconds,
      image_counts: capabilities.image_counts.map(String),
      max_images: capabilities.max_images,
      pricing_config_json: stringifyJson(model.pricing_config),
      metadata_json: stringifyJson(model.metadata)
    });
    setModelModalOpen(true);
  };

  const openAdjustWallet = (wallet: AiWallet, direction: "credit" | "debit") => {
    setSelectedWallet(wallet);
    walletForm.setFieldsValue({
      direction,
      amount: 0,
      reason: direction === "credit" ? "后台充值" : "后台扣减"
    });
    setWalletModalOpen(true);
  };

  const openLedger = (wallet: AiWallet) => {
    setSelectedWallet(wallet);
    setLedgerModalOpen(true);
  };

  const openGenerationJobDetail = (job: AiGenerationJob) => {
    setSelectedGenerationJob(job);
    setGenerationJobDetailOpen(true);
  };

  const openWalletQuota = (wallet: AiWallet) => {
    setSelectedWallet(wallet);
    walletQuotaForm.setFieldsValue({
      daily_spend_limit:
        wallet.daily_spend_limit_minor == null
          ? null
          : minorToMoneyNumber(wallet.daily_spend_limit_minor)
    });
    setWalletQuotaModalOpen(true);
  };

  const openCreateApiKey = () => {
    setGeneratedApiKey(null);
    apiKeyForm.setFieldsValue({
      customer_id: undefined,
      name: "默认 SDK Key",
      daily_spend_limit: null
    });
    setApiKeyModalOpen(true);
  };

  const openEditApiKey = (apiKey: AiApiKey) => {
    setEditingApiKey(apiKey);
    apiKeyEditForm.setFieldsValue({
      name: apiKey.name,
      daily_spend_limit:
        apiKey.daily_spend_limit_minor == null
          ? null
          : minorToMoneyNumber(apiKey.daily_spend_limit_minor)
    });
    setApiKeyEditModalOpen(true);
  };

  const providerColumns: ColumnsType<AiProvider> = [
    {
      title: "渠道",
      dataIndex: "name",
      key: "name",
      width: 360,
      render: (value: string, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text ellipsis title={value}>
            {value}
          </Typography.Text>
          <Typography.Text ellipsis title={record.base_url} type="secondary">
            {record.base_url}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "类型",
      dataIndex: "kind",
      key: "kind",
      width: 130,
      render: (value: AiProviderKind) => <Tag>{providerKindLabel(value)}</Tag>
    },
    {
      title: "状态",
      dataIndex: "enabled",
      key: "enabled",
      width: 90,
      render: (value: boolean) => (
        <Tag color={value ? "green" : "default"}>{value ? "启用" : "停用"}</Tag>
      )
    },
    {
      title: "密钥",
      dataIndex: "secret_configured",
      key: "secret_configured",
      width: 90,
      render: (value: boolean) => (
        <Tag color={value ? "blue" : "red"}>{value ? "已配置" : "未配置"}</Tag>
      )
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
      width: 90,
      render: (_, record) => (
        <Tooltip title="编辑">
          <Button
            aria-label={`编辑渠道 ${record.name}`}
            size="small"
            icon={<Pencil size={14} />}
            disabled={!canUpdateProvider}
            onClick={() => openEditProvider(record)}
          />
        </Tooltip>
      )
    }
  ];

  const modelColumns: ColumnsType<AiModel> = [
    {
      title: "模型",
      dataIndex: "code",
      key: "code",
      width: 280,
      render: (value: string, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text copyable ellipsis title={value}>
            {value}
          </Typography.Text>
          <Typography.Text ellipsis title={record.name} type="secondary">
            {record.name}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "类型",
      dataIndex: "modality",
      key: "modality",
      width: 90,
      render: (value: AiModelModality) => <Tag>{modalityLabel(value)}</Tag>
    },
    {
      title: "渠道",
      dataIndex: "provider_name",
      key: "provider_name",
      width: 220,
      render: (_, record) => (
        <Typography.Text ellipsis title={record.provider_name ?? record.provider_model ?? "-"}>
          {record.provider_name ?? record.provider_model ?? "-"}
        </Typography.Text>
      )
    },
    {
      title: "价格",
      key: "prices",
      width: 320,
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text>{billingPriceSummary(record)}</Typography.Text>
          <Typography.Text type="secondary">
            计费方式 {billingModeLabel(record.billing_mode)}
          </Typography.Text>
          <Typography.Text type="secondary">
            每日限额 {limitText(record.daily_spend_limit_minor, record.currency)}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "可用能力",
      key: "capabilities",
      width: 360,
      render: (_, record) => (
        <Typography.Text type="secondary">{modelCapabilitiesSummary(record)}</Typography.Text>
      )
    },
    {
      title: "状态",
      dataIndex: "enabled",
      key: "enabled",
      width: 90,
      render: (value: boolean) => (
        <Tag color={value ? "green" : "default"}>{value ? "启用" : "停用"}</Tag>
      )
    },
    {
      title: "操作",
      key: "actions",
      width: 90,
      render: (_, record) => (
        <Tooltip title="编辑">
          <Button
            aria-label={`编辑模型 ${record.code}`}
            size="small"
            icon={<Pencil size={14} />}
            disabled={!canUpdateModel}
            onClick={() => openEditModel(record)}
          />
        </Tooltip>
      )
    }
  ];

  const walletColumns: ColumnsType<AiWallet> = [
    {
      title: "客户",
      dataIndex: "customer_email",
      key: "customer_email",
      width: 380,
      render: (value: string, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text ellipsis title={record.customer_name || value}>
            {record.customer_name || value}
          </Typography.Text>
          <Typography.Text ellipsis title={value} type="secondary">
            {value}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "AI 状态",
      dataIndex: "ai_enabled",
      key: "ai_enabled",
      width: 100,
      render: (_: boolean | undefined, record) => {
        const enabled = isAiWalletEnabled(record);

        return <Tag color={enabled ? "success" : "error"}>{enabled ? "可用" : "已冻结"}</Tag>;
      }
    },
    {
      title: "余额",
      dataIndex: "balance_minor",
      key: "balance_minor",
      width: 150,
      render: (value: number, record) => money(value, record.currency)
    },
    {
      title: "冻结",
      dataIndex: "held_minor",
      key: "held_minor",
      width: 150,
      render: (value: number, record) => money(value, record.currency)
    },
    {
      title: "可用",
      dataIndex: "available_minor",
      key: "available_minor",
      width: 150,
      render: (value: number, record) => money(value, record.currency)
    },
    {
      title: "每日限额",
      dataIndex: "daily_spend_limit_minor",
      key: "daily_spend_limit_minor",
      width: 150,
      render: (value: number | null | undefined, record) => limitText(value, record.currency)
    },
    {
      title: "更新时间",
      dataIndex: "updated_at",
      key: "updated_at",
      width: 180,
      render: (value?: string | null) => (value ? dateTime(value) : "-")
    },
    {
      title: "操作",
      key: "actions",
      width: 340,
      render: (_, record) => (
        <Space size={6}>
          <Button
            size="small"
            icon={<Coins size={14} />}
            disabled={!canUpdateWallet}
            onClick={() => openAdjustWallet(record, "credit")}
          >
            充值
          </Button>
          <Button
            size="small"
            danger
            disabled={!canUpdateWallet}
            onClick={() => openAdjustWallet(record, "debit")}
          >
            扣减
          </Button>
          <Tooltip title="流水">
            <Button
              aria-label={`查看余额流水 ${record.customer_email}`}
              size="small"
              icon={<History size={14} />}
              onClick={() => openLedger(record)}
            />
          </Tooltip>
          <Tooltip title="每日限额">
            <Button
              aria-label={`设置每日限额 ${record.customer_email}`}
              size="small"
              icon={<Pencil size={14} />}
              disabled={!canUpdateWallet}
              onClick={() => openWalletQuota(record)}
            />
          </Tooltip>
          <Tooltip
            title={
              isAiWalletEnabled(record)
                ? "立即冻结该客户的 AI 调用权限"
                : "立即恢复该客户的 AI 调用权限"
            }
          >
            <Button
              size="small"
              danger={isAiWalletEnabled(record)}
              icon={isAiWalletEnabled(record) ? <Ban size={14} /> : <RefreshCw size={14} />}
              disabled={!canUpdateWallet}
              loading={walletAccessMutation.isPending}
              onClick={() =>
                walletAccessMutation.mutate({
                  wallet: record,
                  ai_enabled: !isAiWalletEnabled(record)
                })
              }
            >
              {isAiWalletEnabled(record) ? "冻结AI" : "恢复AI"}
            </Button>
          </Tooltip>
        </Space>
      )
    }
  ];

  const ledgerColumns: ColumnsType<AiWalletLedgerEntry> = [
    {
      title: "类型",
      dataIndex: "entry_type",
      key: "entry_type",
      width: 90,
      render: (value: string) => <Tag>{ledgerTypeLabel(value)}</Tag>
    },
    {
      title: "金额",
      dataIndex: "amount_minor",
      key: "amount_minor",
      width: 130,
      render: (value: number) => (
        <Typography.Text type={value < 0 ? "danger" : "success"}>
          {money(value, selectedWallet?.currency ?? "CNY")}
        </Typography.Text>
      )
    },
    {
      title: "余额",
      dataIndex: "balance_after_minor",
      key: "balance_after_minor",
      width: 130,
      render: (value: number) => money(value, selectedWallet?.currency ?? "CNY")
    },
    {
      title: "原因",
      dataIndex: "reason",
      key: "reason",
      width: 260,
      render: (value: string) => (
        <Typography.Text ellipsis title={value}>
          {value}
        </Typography.Text>
      )
    },
    {
      title: "时间",
      dataIndex: "created_at",
      key: "created_at",
      width: 180,
      render: (value: string) => dateTime(value)
    }
  ];

  const apiKeyColumns: ColumnsType<AiApiKey> = [
    {
      title: "客户",
      dataIndex: "customer_email",
      key: "customer_email",
      width: 380,
      render: (value: string, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text ellipsis title={record.customer_name || value}>
            {record.customer_name || value}
          </Typography.Text>
          <Typography.Text ellipsis title={value} type="secondary">
            {value}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "名称",
      dataIndex: "name",
      key: "name",
      width: 180,
      render: (value: string) => (
        <Typography.Text ellipsis title={value}>
          {value}
        </Typography.Text>
      )
    },
    {
      title: "Key 前缀",
      dataIndex: "key_prefix",
      key: "key_prefix",
      width: 180,
      render: (value: string) => <Typography.Text code>{value}</Typography.Text>
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 90,
      render: (value: string) => (
        <Tag color={value === "active" ? "green" : "default"}>
          {value === "active" ? "启用" : "已吊销"}
        </Tag>
      )
    },
    {
      title: "最近使用",
      dataIndex: "last_used_at",
      key: "last_used_at",
      width: 180,
      render: (value?: string | null) => (value ? dateTime(value) : "-")
    },
    {
      title: "每日限额",
      dataIndex: "daily_spend_limit_minor",
      key: "daily_spend_limit_minor",
      width: 150,
      render: (value: number | null | undefined) => limitText(value, "CNY")
    },
    {
      title: "创建时间",
      dataIndex: "created_at",
      key: "created_at",
      width: 180,
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 170,
      render: (_, record) => (
        <Space size={6}>
          <Tooltip title="编辑">
            <Button
              aria-label={`编辑 API Key ${record.name}`}
              size="small"
              icon={<Pencil size={14} />}
              disabled={!canUpdateApiKey}
              onClick={() => openEditApiKey(record)}
            />
          </Tooltip>
          {record.status === "active" ? (
            <ConfirmActionButton
              title="吊销 AI API Key"
              description="吊销后，正在使用这个 Key 的客户端将无法继续调用 AI 网关。"
              confirmText="吊销"
              okText="吊销"
              loading={revokeApiKeyMutation.isPending}
              buttonProps={{
                size: "small",
                danger: true,
                disabled: !canUpdateApiKey,
                icon: <Ban size={14} />
              }}
              onConfirm={() => revokeApiKeyMutation.mutate(record.id)}
            >
              吊销
            </ConfirmActionButton>
          ) : null}
        </Space>
      )
    }
  ];

  const usageColumns: ColumnsType<AiUsageRecord> = [
    {
      title: "客户",
      dataIndex: "customer_email",
      key: "customer_email",
      width: 340,
      render: (value: string | null | undefined, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text ellipsis title={record.customer_name || value || "-"}>
            {record.customer_name || value || "-"}
          </Typography.Text>
          {value ? (
            <Typography.Text ellipsis title={value} type="secondary">
              {value}
            </Typography.Text>
          ) : null}
        </Space>
      )
    },
    {
      title: "模型",
      dataIndex: "model_code",
      key: "model_code",
      width: 240,
      render: (value: string | null | undefined, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text ellipsis title={value ?? "-"}>
            {value ?? "-"}
          </Typography.Text>
          <Typography.Text ellipsis title={record.provider_name ?? "-"} type="secondary">
            {record.provider_name ?? "-"}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 100,
      render: (value: string) => <Tag>{usageStatusLabel(value)}</Tag>
    },
    {
      title: "Token",
      key: "tokens",
      width: 170,
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text>输入 {record.prompt_tokens ?? "-"}</Typography.Text>
          <Typography.Text>输出 {record.completion_tokens ?? "-"}</Typography.Text>
          <Typography.Text type="secondary">总计 {record.total_tokens ?? "-"}</Typography.Text>
        </Space>
      )
    },
    {
      title: "金额",
      key: "amounts",
      width: 180,
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text>扣费 {money(record.charged_minor, record.currency)}</Typography.Text>
          <Typography.Text type="secondary">
            释放/退款 {money(record.refunded_minor, record.currency)}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "三方状态",
      dataIndex: "provider_status",
      key: "provider_status",
      width: 110,
      render: (value?: string | null) => value ?? "-"
    },
    {
      title: "创建时间",
      dataIndex: "created_at",
      key: "created_at",
      width: 180,
      render: (value: string) => dateTime(value)
    }
  ];

  const generationJobColumns: ColumnsType<AiGenerationJob> = [
    {
      title: "客户",
      dataIndex: "customer_email",
      key: "customer_email",
      width: 340,
      render: (value: string | null | undefined, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text ellipsis title={record.customer_name || value || "-"}>
            {record.customer_name || value || "-"}
          </Typography.Text>
          {value ? (
            <Typography.Text ellipsis title={value} type="secondary">
              {value}
            </Typography.Text>
          ) : null}
        </Space>
      )
    },
    {
      title: "任务",
      key: "job",
      width: 260,
      render: (_, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Space size={6}>
            <Tag>{generationJobTypeLabel(record.job_type)}</Tag>
            <Typography.Text ellipsis title={record.model_code ?? "-"}>
              {record.model_code ?? "-"}
            </Typography.Text>
          </Space>
          <Typography.Text ellipsis title={record.provider_name ?? "-"} type="secondary">
            {record.provider_name ?? "-"}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 130,
      render: (value: AiGenerationJobStatus) => <Tag>{generationJobStatusLabel(value)}</Tag>
    },
    {
      title: "三方任务",
      dataIndex: "provider_job_id",
      key: "provider_job_id",
      width: 260,
      render: (value: string | null | undefined, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text copyable={Boolean(value)} ellipsis title={value ?? "-"}>
            {value ?? "-"}
          </Typography.Text>
          <Typography.Text type="secondary">状态 {record.provider_status ?? "-"}</Typography.Text>
        </Space>
      )
    },
    {
      title: "计费",
      key: "billing",
      width: 220,
      render: (_, record) => (
        <Space direction="vertical" size={0}>
          <Typography.Text>
            {billingModeLabels[record.charge_mode as AiModelBillingMode] ?? record.charge_mode}
            {" x "}
            {record.quantity}
          </Typography.Text>
          <Typography.Text type="secondary">
            预扣 {money(record.held_minor, record.currency)}
          </Typography.Text>
          <Typography.Text type="secondary">
            扣费 {money(record.charged_minor, record.currency)}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "素材",
      dataIndex: "asset_urls",
      key: "asset_urls",
      width: 280,
      render: (value: string[]) =>
        value?.length ? (
          <Space className="ai-stacked-cell" direction="vertical" size={0}>
            {value.slice(0, 2).map((url) => (
              <Typography.Text key={url} copyable ellipsis title={url}>
                {url}
              </Typography.Text>
            ))}
            {value.length > 2 ? (
              <Typography.Text type="secondary">还有 {value.length - 2} 个素材</Typography.Text>
            ) : null}
          </Space>
        ) : (
          "-"
        )
    },
    {
      title: "异常",
      dataIndex: "failure_reason",
      key: "failure_reason",
      width: 260,
      render: (value?: string | null) =>
        value ? (
          <Typography.Text type="danger" ellipsis title={value}>
            {value}
          </Typography.Text>
        ) : (
          "-"
        )
    },
    {
      title: "创建时间",
      dataIndex: "created_at",
      key: "created_at",
      width: 180,
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      fixed: "right",
      width: canUpdateJob ? 430 : 100,
      render: (_, record) => (
        <Space size={8} wrap={false}>
          <Tooltip title="查看详情">
            <Button
              size="small"
              icon={<Eye size={14} />}
              onClick={() => openGenerationJobDetail(record)}
            >
              查看
            </Button>
          </Tooltip>
          {canUpdateJob ? (
            <>
              <ConfirmActionButton
                title="重新查询第三方任务"
                description="任务会重新进入查询队列，状态以三方接口返回为准。"
                confirmText="重新查询"
                okText="重新查询"
                loading={retryJobPollMutation.isPending}
                buttonProps={{
                  size: "small",
                  icon: <RefreshCw size={14} />,
                  disabled: !canRetryGenerationJobPoll(record)
                }}
                onConfirm={() => retryJobPollMutation.mutate(record)}
              >
                重新查询
              </ConfirmActionButton>
              <ConfirmActionButton
                title="重新缓存生成素材"
                description="会使用三方返回结果重新下载并缓存素材，已扣费任务不能重复缓存。"
                confirmText="重新缓存"
                okText="重新缓存"
                loading={retryJobCacheMutation.isPending}
                buttonProps={{
                  size: "small",
                  icon: <RotateCw size={14} />,
                  disabled: !canRetryGenerationJobCache(record)
                }}
                onConfirm={() => retryJobCacheMutation.mutate(record)}
              >
                重新缓存
              </ConfirmActionButton>
              <ConfirmActionButton
                title="标记失败并释放预扣"
                description="仅适用于未结算任务，已扣费任务请走人工退款。"
                confirmText="标记失败"
                okText="标记失败"
                loading={failReleaseJobMutation.isPending}
                buttonProps={{
                  danger: true,
                  size: "small",
                  icon: <Ban size={14} />,
                  disabled: !canFailReleaseGenerationJob(record)
                }}
                onConfirm={() => failReleaseJobMutation.mutate(record)}
              >
                标记失败
              </ConfirmActionButton>
              <ConfirmActionButton
                title="人工退款"
                description="会把已扣费用退回客户 AI 余额，并记录退款流水。"
                confirmText="人工退款"
                okText="退款"
                loading={refundJobMutation.isPending}
                buttonProps={{
                  danger: true,
                  size: "small",
                  icon: <Undo2 size={14} />,
                  disabled: !canRefundGenerationJob(record)
                }}
                onConfirm={() => refundJobMutation.mutate(record)}
              >
                退款
              </ConfirmActionButton>
            </>
          ) : null}
        </Space>
      )
    }
  ];

  const assetColumns: ColumnsType<AiAsset> = [
    {
      title: "客户",
      dataIndex: "customer_email",
      key: "customer_email",
      width: 340,
      render: (value: string | null | undefined, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text ellipsis title={record.customer_name || value || "-"}>
            {record.customer_name || value || "-"}
          </Typography.Text>
          {value ? (
            <Typography.Text ellipsis title={value} type="secondary">
              {value}
            </Typography.Text>
          ) : null}
        </Space>
      )
    },
    {
      title: "素材",
      dataIndex: "asset_type",
      key: "asset_type",
      width: 120,
      render: (value: AiAssetType, record) => (
        <Space direction="vertical" size={0}>
          <Tag>{assetTypeLabel(value)}</Tag>
          <Typography.Text type="secondary">{record.mime_type ?? "-"}</Typography.Text>
        </Space>
      )
    },
    {
      title: "模型",
      key: "model",
      width: 240,
      render: (_, record) => (
        <Space className="ai-stacked-cell" direction="vertical" size={0}>
          <Typography.Text ellipsis title={record.model_code ?? "-"}>
            {record.model_code ?? "-"}
          </Typography.Text>
          <Typography.Text ellipsis title={record.provider_name ?? "-"} type="secondary">
            {record.provider_name ?? "-"}
          </Typography.Text>
        </Space>
      )
    },
    {
      title: "状态",
      dataIndex: "status",
      key: "status",
      width: 100,
      render: (value: AiAssetStatus) => <Tag>{assetStatusLabel(value)}</Tag>
    },
    {
      title: "大小",
      dataIndex: "file_size",
      key: "file_size",
      width: 110,
      render: (value?: number | null) => formatBytes(value)
    },
    {
      title: "地址",
      dataIndex: "public_url",
      key: "public_url",
      width: 320,
      render: (value?: string | null) =>
        value ? (
          <Typography.Text copyable ellipsis>
            {value}
          </Typography.Text>
        ) : (
          "-"
        )
    },
    {
      title: "创建时间",
      dataIndex: "created_at",
      key: "created_at",
      width: 180,
      render: (value: string) => dateTime(value)
    },
    {
      title: "操作",
      key: "actions",
      width: 100,
      render: (_, record) =>
        record.status !== "deleted" ? (
          <ConfirmActionButton
            title="删除缓存素材"
            description="删除后客户端将无法继续通过平台地址访问这个素材。"
            confirmText="删除"
            okText="删除"
            loading={deleteAssetMutation.isPending}
            buttonProps={{
              size: "small",
              danger: true,
              disabled: !canDeleteAsset,
              icon: <Trash2 size={14} />
            }}
            onConfirm={() => deleteAssetMutation.mutate(record.id)}
          >
            删除
          </ConfirmActionButton>
        ) : (
          "-"
        )
    }
  ];

  const providerOptions = (providersQuery.data?.items ?? []).map((provider) => ({
    value: provider.id,
    label: provider.name
  }));

  const customerOptions = (walletsQuery.data?.items ?? []).map((wallet) => ({
    value: wallet.customer_id,
    label: wallet.customer_name
      ? `${wallet.customer_name} <${wallet.customer_email}>`
      : wallet.customer_email
  }));

  const refreshCurrentSection = () => {
    switch (currentSection) {
      case "providers":
        providersQuery.refetch();
        return;
      case "models":
        modelsQuery.refetch();
        return;
      case "wallets":
        walletsQuery.refetch();
        return;
      case "jobs":
        generationJobsQuery.refetch();
        return;
      case "usage":
        usageRecordsQuery.refetch();
        return;
      case "assets":
        assetsQuery.refetch();
        return;
    }
  };

  return (
    <section className="workspace-page ai-billing-page">
      <div className="page-heading">
        <div>
          <Typography.Title level={2}>{heading.title}</Typography.Title>
          <Typography.Text type="secondary">{heading.subtitle}</Typography.Text>
        </div>
        <Space className="page-heading-actions">
          <HistoryToggle checked={includeHistory} onChange={setIncludeHistory} />
          <Button
            aria-label={`刷新${heading.title}数据`}
            icon={<RefreshCw size={16} />}
            onClick={refreshCurrentSection}
          />
          {currentSection === "providers" ? (
            <Button
              type="primary"
              icon={<Plus size={16} />}
              disabled={!canUpdateProvider}
              onClick={openCreateProvider}
            >
              新增渠道
            </Button>
          ) : null}
          {currentSection === "models" ? (
            <Button
              type="primary"
              icon={<Plus size={16} />}
              disabled={!canUpdateModel}
              onClick={openCreateModel}
            >
              新增模型
            </Button>
          ) : null}
        </Space>
      </div>

      {providersQuery.error ||
      modelsQuery.error ||
      walletsQuery.error ||
      (showOpenApiKeyManagement && apiKeysQuery.error) ||
      generationJobsQuery.error ||
      usageRecordsQuery.error ||
      assetsQuery.error ? (
        <Alert
          type="error"
          message={
            tApiError(
              providersQuery.error ||
                modelsQuery.error ||
                walletsQuery.error ||
                (showOpenApiKeyManagement && apiKeysQuery.error) ||
                generationJobsQuery.error ||
                usageRecordsQuery.error ||
                assetsQuery.error
            ) ??
            "接口计费数据加载失败"
          }
        />
      ) : null}
      {providerMutation.error ||
      modelMutation.error ||
      walletMutation.error ||
      walletQuotaMutation.error ||
      walletAccessMutation.error ||
      (showOpenApiKeyManagement && apiKeyMutation.error) ||
      (showOpenApiKeyManagement && updateApiKeyMutation.error) ||
      (showOpenApiKeyManagement && revokeApiKeyMutation.error) ||
      deleteAssetMutation.error ? (
        <Alert
          type="error"
          message={
            tApiError(
              providerMutation.error ||
                modelMutation.error ||
                walletMutation.error ||
                walletQuotaMutation.error ||
                walletAccessMutation.error ||
                (showOpenApiKeyManagement && apiKeyMutation.error) ||
                (showOpenApiKeyManagement && updateApiKeyMutation.error) ||
                (showOpenApiKeyManagement && revokeApiKeyMutation.error) ||
                deleteAssetMutation.error
            ) ??
            "接口计费保存失败"
          }
        />
      ) : null}

      {currentSection === "providers" ? (
        <>
          <Table
            rowKey="id"
            loading={providersQuery.isLoading}
            columns={providerColumns}
            dataSource={providersQuery.data?.items ?? []}
            pagination={false}
            scroll={AI_TABLE_SCROLL}
            locale={{ emptyText: "暂无数据" }}
          />
        </>
      ) : null}

      {currentSection === "models" ? (
        <>
          <Table
            rowKey="id"
            loading={modelsQuery.isLoading}
            columns={modelColumns}
            dataSource={modelsQuery.data?.items ?? []}
            pagination={false}
            scroll={AI_TABLE_SCROLL}
            locale={{ emptyText: "暂无数据" }}
          />
        </>
      ) : null}

      {currentSection === "wallets" ? (
        <Table
          rowKey="customer_id"
          loading={walletsQuery.isLoading}
          columns={walletColumns}
          dataSource={walletsQuery.data?.items ?? []}
          pagination={false}
          scroll={AI_TABLE_SCROLL}
          locale={{ emptyText: "暂无数据" }}
        />
      ) : null}

      {showOpenApiKeyManagement ? (
        <div hidden>
          <Button
            type="primary"
            icon={<KeyRound size={16} />}
            disabled={!canUpdateApiKey}
            onClick={openCreateApiKey}
          >
            生成接口 Key
          </Button>
          <Table
            rowKey="id"
            loading={apiKeysQuery.isLoading}
            columns={apiKeyColumns}
            dataSource={apiKeysQuery.data?.items ?? []}
            pagination={false}
            scroll={AI_TABLE_SCROLL}
            locale={{ emptyText: "暂无数据" }}
          />
        </div>
      ) : null}

      {currentSection === "jobs" ? (
        <Table
          rowKey="id"
          loading={generationJobsQuery.isLoading}
          columns={generationJobColumns}
          dataSource={generationJobsQuery.data?.items ?? []}
          pagination={false}
          scroll={AI_TABLE_SCROLL}
          locale={{ emptyText: "暂无数据" }}
        />
      ) : null}

      {currentSection === "usage" ? (
        <Table
          rowKey="id"
          loading={usageRecordsQuery.isLoading}
          columns={usageColumns}
          dataSource={usageRecordsQuery.data?.items ?? []}
          pagination={false}
          scroll={AI_TABLE_SCROLL}
          locale={{ emptyText: "暂无数据" }}
        />
      ) : null}

      {currentSection === "assets" ? (
        <Table
          rowKey="id"
          loading={assetsQuery.isLoading}
          columns={assetColumns}
          dataSource={assetsQuery.data?.items ?? []}
          pagination={false}
          scroll={AI_TABLE_SCROLL}
          locale={{ emptyText: "暂无数据" }}
        />
      ) : null}

      <Modal
        title={editingProvider ? "编辑 AI 渠道" : "新增 AI 渠道"}
        open={providerModalOpen}
        onCancel={() => {
          setProviderModalOpen(false);
          setEditingProvider(null);
        }}
        onOk={() => providerForm.submit()}
        confirmLoading={providerMutation.isPending}
        width={760}
        destroyOnHidden
      >
        <Form<ProviderFormValues>
          form={providerForm}
          layout="vertical"
          onValuesChange={(changedValues) => {
            if (!editingProvider && "kind" in changedValues) {
              const kind = changedValues.kind as AiProviderKind;
              providerForm.setFieldValue(
                "config_json",
                stringifyJson(defaultProviderConfigForKind(kind))
              );
            }
          }}
          onFinish={(values) => providerMutation.mutate(values)}
        >
          <div className="settings-grid-inner">
            <Form.Item name="name" label="名称" rules={[{ required: true }]}>
              <Input autoComplete="off" />
            </Form.Item>
            <Form.Item name="kind" label="类型" rules={[{ required: true }]}>
              <Select
                disabled={Boolean(editingProvider)}
                options={providerKindOptions}
                onChange={(kind: AiProviderKind) => {
                  if (!editingProvider) {
                    providerForm.setFieldValue(
                      "config_json",
                      stringifyJson(defaultProviderConfigForKind(kind))
                    );
                  }
                }}
              />
            </Form.Item>
            <Form.Item name="enabled" label="启用" valuePropName="checked">
              <Switch />
            </Form.Item>
          </div>
          <Form.Item
            name="base_url"
            label="接口地址"
            rules={[{ required: true }, { type: "url", message: "URL 格式不正确" }]}
          >
            <Input autoComplete="url" placeholder="https://api.example.com/v1" />
          </Form.Item>
          <Form.Item
            name="api_key"
            label="API Key"
            rules={[{ required: !editingProvider, message: "请输入 API Key" }]}
          >
            <Input.Password
              autoComplete="new-password"
              placeholder={editingProvider?.secret_configured ? "已配置" : ""}
            />
          </Form.Item>
          <Form.Item
            name="config_json"
            label="渠道配置 JSON"
            extra="用于保存超时、查询路径和平台模型能力模板；密钥不要写在这里。"
            rules={[{ validator: validateJsonField }]}
          >
            <Input.TextArea className="settings-json-editor" rows={10} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={editingModel ? "编辑模型商品" : "新增模型商品"}
        open={modelModalOpen}
        onCancel={() => {
          setModelModalOpen(false);
          setEditingModel(null);
        }}
        onOk={() => modelForm.submit()}
        confirmLoading={modelMutation.isPending}
        width={820}
        destroyOnHidden
      >
        <Form<ModelFormValues>
          form={modelForm}
          layout="vertical"
          onValuesChange={(changedValues) => {
            if ("modality" in changedValues) {
              const modality = changedValues.modality as AiModelModality;
              const provider = (providersQuery.data?.items ?? []).find(
                (item) => item.id === modelForm.getFieldValue("provider_id")
              );
              const template = templatesForProviderAndModality(provider, modality)[0];
              modelForm.setFieldValue("billing_mode", defaultBillingModeForModality(modality));
              modelForm.setFieldsValue(defaultCapabilitiesForModality(modality));
              if (template) {
                applyProviderModelTemplate(template, {
                  applyName: shouldReplaceModelNameWithTemplate(
                    provider,
                    modelForm.getFieldValue("name")
                  )
                });
              } else {
                clearProviderModelTemplate(modality);
              }
            }
            if ("provider_id" in changedValues) {
              const provider = (providersQuery.data?.items ?? []).find(
                (item) => item.id === changedValues.provider_id
              );
              const template = templatesForProviderAndModality(provider, selectedModelModality)[0];
              if (template) {
                applyProviderModelTemplate(template, {
                  applyName: shouldReplaceModelNameWithTemplate(
                    provider,
                    modelForm.getFieldValue("name")
                  )
                });
              } else {
                clearProviderModelTemplate(selectedModelModality);
              }
            }
            if ("provider_model" in changedValues) {
              const provider = (providersQuery.data?.items ?? []).find(
                (item) => item.id === modelForm.getFieldValue("provider_id")
              );
              const template = findProviderModelTemplate(
                provider,
                selectedModelModality,
                changedValues.provider_model
              );
              applyProviderModelTemplate(template, { applyName: true });
            }
          }}
          onFinish={(values) => modelMutation.mutate(values)}
        >
          <div className="settings-grid-inner">
            <Form.Item name="code" label="模型代码" rules={[{ required: true }]}>
              <Input disabled={Boolean(editingModel)} />
            </Form.Item>
            <Form.Item name="name" label="显示名称" rules={[{ required: true }]}>
              <Input />
            </Form.Item>
            <Form.Item name="modality" label="类型" rules={[{ required: true }]}>
              <Select options={modalityOptions} />
            </Form.Item>
            <Form.Item name="enabled" label="启用" valuePropName="checked">
              <Switch />
            </Form.Item>
          </div>
          <div className="settings-grid-inner">
            <Form.Item name="provider_id" label="渠道">
              <Select allowClear options={providerOptions} />
            </Form.Item>
            <Form.Item name="provider_model" label="三方模型名">
              <AutoComplete
                options={providerModelOptions}
                placeholder={
                  providerModelOptions.length > 0 ? "选择平台模板或手动输入" : "手动输入三方模型名"
                }
                filterOption={(inputValue, option) =>
                  providerModelOptionMatchesCurrentValue(
                    inputValue,
                    modelForm.getFieldValue("provider_model")
                  ) ||
                  String(option?.value ?? "")
                    .toLowerCase()
                    .includes(inputValue.toLowerCase()) ||
                  String(option?.label ?? "")
                    .toLowerCase()
                    .includes(inputValue.toLowerCase())
                }
              />
            </Form.Item>
            <Form.Item name="currency" label="币种" rules={[{ required: true }]}>
              <Input maxLength={3} />
            </Form.Item>
            <Form.Item name="billing_mode" label="计费方式" rules={[{ required: true }]}>
              <Select options={billingModeOptions} disabled={billingModeOptions.length === 1} />
            </Form.Item>
          </div>
          <div className="settings-grid-inner">
            {selectedBillingMode === "token" ? (
              <>
                <MoneyFormItem name="input_1k_price" label="输入 / 1K token" />
                <MoneyFormItem name="output_1k_price" label="输出 / 1K token" />
              </>
            ) : null}
            {selectedBillingMode === "per_image" ? (
              <MoneyFormItem name="image_price" label="每张图片" />
            ) : null}
            {selectedBillingMode === "video_per_second" ? (
              <MoneyFormItem name="second_price" label="每秒视频" />
            ) : null}
            {selectedBillingMode === "video_per_request" ? (
              <MoneyFormItem name="request_price" label="每次视频请求" />
            ) : null}
            {selectedBillingMode === "audio_per_second" ? (
              <MoneyFormItem name="second_price" label="每秒音频" />
            ) : null}
            {selectedBillingMode === "audio_per_minute" ? (
              <MoneyFormItem name="minute_price" label="每分钟音频" />
            ) : null}
            {selectedBillingMode === "audio_per_request" ? (
              <MoneyFormItem name="request_price" label="每次音频请求" />
            ) : null}
            <OptionalMoneyFormItem name="daily_spend_limit" label="每日限额" />
          </div>
          {supportsVisualCapabilities(selectedModelModality) ? (
            <>
              <Typography.Title level={5}>模型能力</Typography.Title>
              <div className="settings-grid-inner">
                <Form.Item name="ratios" label="支持比例">
                  <Select
                    mode="tags"
                    tokenSeparators={[",", "，", "\n"]}
                    options={mergedSelectOptions(
                      ratioOptionsForModality(selectedModelModality).map((option) => option.value),
                      modelForm.getFieldValue("ratios")
                    )}
                    placeholder="例如 16:9、9:16"
                  />
                </Form.Item>
                <Form.Item name="resolutions" label="支持分辨率/尺寸">
                  <Select
                    mode="tags"
                    tokenSeparators={[",", "，", "\n"]}
                    options={mergedSelectOptions(
                      resolutionOptionsForModality(selectedModelModality).map((option) => option.value),
                      modelForm.getFieldValue("resolutions")
                    )}
                    placeholder="例如 720p、1024x1024"
                  />
                </Form.Item>
              </div>
            </>
          ) : null}
          {selectedModelModality === "image" || selectedBillingMode === "per_image" ? (
            <div className="settings-grid-inner">
              <Form.Item name="image_counts" label="允许张数">
                <Select
                  mode="tags"
                  tokenSeparators={[",", "，", "\n"]}
                  options={mergedSelectOptions(defaultImageCounts, modelForm.getFieldValue("image_counts"))}
                  placeholder="例如 1、2、4"
                />
              </Form.Item>
              <Form.Item name="max_images" label="单次最多张数">
                <InputNumber min={1} max={10} precision={0} className="form-number" />
              </Form.Item>
            </div>
          ) : null}
          {selectedModelModality === "video" ||
          selectedBillingMode === "video_per_second" ||
          selectedBillingMode === "video_per_request" ? (
            <div className="settings-grid-inner">
              <Form.Item name="durations" label="允许时长（秒）">
                <Select
                  mode="tags"
                  tokenSeparators={[",", "，", "\n"]}
                  options={mergedSelectOptions(
                    defaultVideoDurations,
                    modelForm.getFieldValue("durations")
                  )}
                  placeholder="例如 5、8、10"
                />
              </Form.Item>
              <Form.Item name="default_duration_seconds" label="默认时长（秒）">
                <InputNumber min={1} max={3600} precision={0} className="form-number" />
              </Form.Item>
            </div>
          ) : null}
          <Form.Item
            name="pricing_config_json"
            label="高级配置 JSON"
            rules={[{ validator: validateJsonField }]}
          >
            <Input.TextArea className="settings-json-editor" rows={5} />
          </Form.Item>
          <Form.Item
            name="metadata_json"
            label="扩展配置 JSON"
            rules={[{ validator: validateJsonField }]}
          >
            <Input.TextArea className="settings-json-editor" rows={8} />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={selectedWallet ? `调整余额：${selectedWallet.customer_email}` : "调整余额"}
        open={walletModalOpen}
        onCancel={() => setWalletModalOpen(false)}
        onOk={() => walletForm.submit()}
        confirmLoading={walletMutation.isPending}
        destroyOnHidden
      >
        <Form<WalletAdjustFormValues>
          form={walletForm}
          layout="vertical"
          onFinish={(values) => walletMutation.mutate(values)}
        >
          <Form.Item name="direction" label="类型" rules={[{ required: true }]}>
            <Select
              options={[
                { value: "credit", label: "充值" },
                { value: "debit", label: "扣减" }
              ]}
            />
          </Form.Item>
          <Form.Item name="amount" label="金额" rules={[{ required: true }]}>
            <InputNumber min={0.01} precision={2} className="form-number" />
          </Form.Item>
          <Form.Item name="reason" label="原因" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={selectedWallet ? `每日限额：${selectedWallet.customer_email}` : "每日限额"}
        open={walletQuotaModalOpen}
        onCancel={() => setWalletQuotaModalOpen(false)}
        onOk={() => walletQuotaForm.submit()}
        confirmLoading={walletQuotaMutation.isPending}
        destroyOnHidden
      >
        <Form<WalletQuotaFormValues>
          form={walletQuotaForm}
          layout="vertical"
          onFinish={(values) => walletQuotaMutation.mutate(values)}
        >
          <Form.Item name="daily_spend_limit" label="每日限额">
            <InputNumber min={0} precision={2} className="form-number" placeholder="留空表示不限" />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={selectedWallet ? `余额流水：${selectedWallet.customer_email}` : "余额流水"}
        open={ledgerModalOpen}
        onCancel={() => setLedgerModalOpen(false)}
        footer={null}
        width={820}
      >
        <Table
          rowKey="id"
          loading={ledgerQuery.isLoading}
          columns={ledgerColumns}
          dataSource={ledgerQuery.data?.items ?? []}
          pagination={false}
          scroll={AI_TABLE_SCROLL}
          locale={{ emptyText: "暂无数据" }}
        />
      </Modal>

      <Modal
        title="生成 AI API Key"
        open={apiKeyModalOpen}
        onCancel={() => {
          setApiKeyModalOpen(false);
          setGeneratedApiKey(null);
          apiKeyForm.resetFields();
        }}
        onOk={() => apiKeyForm.submit()}
        okButtonProps={{ disabled: Boolean(generatedApiKey) }}
        confirmLoading={apiKeyMutation.isPending}
        destroyOnHidden
      >
        <Space direction="vertical" size={12} className="settings-stack">
          {generatedApiKey ? (
            <Alert
              type="success"
              showIcon
              message="请立即复制保存，关闭后不会再次显示明文 Key。"
              description={
                <Typography.Paragraph copyable className="api-key-preview">
                  {generatedApiKey}
                </Typography.Paragraph>
              }
            />
          ) : null}
          <Form<ApiKeyFormValues>
            form={apiKeyForm}
            layout="vertical"
            onFinish={(values) => apiKeyMutation.mutate(values)}
          >
            <Form.Item name="customer_id" label="客户" rules={[{ required: true }]}>
              <Select
                showSearch
                options={customerOptions}
                optionFilterProp="label"
                placeholder="选择客户"
              />
            </Form.Item>
            <Form.Item name="name" label="名称" rules={[{ required: true }]}>
              <Input placeholder="例如：生产环境 SDK Key" />
            </Form.Item>
            <Form.Item name="daily_spend_limit" label="每日限额">
              <InputNumber min={0} precision={2} className="form-number" placeholder="留空表示不限" />
            </Form.Item>
          </Form>
        </Space>
      </Modal>

      <Modal
        title={editingApiKey ? `编辑 API Key：${editingApiKey.key_prefix}` : "编辑 API Key"}
        open={apiKeyEditModalOpen}
        onCancel={() => {
          setApiKeyEditModalOpen(false);
          setEditingApiKey(null);
        }}
        onOk={() => apiKeyEditForm.submit()}
        confirmLoading={updateApiKeyMutation.isPending}
        destroyOnHidden
      >
        <Form<ApiKeyEditFormValues>
          form={apiKeyEditForm}
          layout="vertical"
          onFinish={(values) => updateApiKeyMutation.mutate(values)}
        >
          <Form.Item name="name" label="名称" rules={[{ required: true }]}>
            <Input />
          </Form.Item>
          <Form.Item name="daily_spend_limit" label="每日限额">
            <InputNumber min={0} precision={2} className="form-number" placeholder="留空表示不限" />
          </Form.Item>
        </Form>
      </Modal>

      <Modal
        title={selectedGenerationJob ? `生成任务：${selectedGenerationJob.id}` : "生成任务详情"}
        open={generationJobDetailOpen}
        onCancel={() => setGenerationJobDetailOpen(false)}
        footer={null}
        width={900}
        destroyOnHidden
      >
        {generationJobDetailQuery.isLoading ? (
          <Typography.Text type="secondary">加载中...</Typography.Text>
        ) : generationJobDetailQuery.data?.job ? (
          <GenerationJobDetailView job={generationJobDetailQuery.data.job} />
        ) : (
          <Typography.Text type="secondary">暂无数据</Typography.Text>
        )}
      </Modal>

    </section>
  );
}

function MoneyFormItem({ name, label }: { name: keyof ModelFormValues; label: string }) {
  return (
    <Form.Item name={name} label={label} rules={[{ required: true }]}>
      <InputNumber min={0} precision={4} className="form-number" />
    </Form.Item>
  );
}

function OptionalMoneyFormItem({ name, label }: { name: keyof ModelFormValues; label: string }) {
  return (
    <Form.Item name={name} label={label}>
      <InputNumber min={0} precision={2} className="form-number" placeholder="留空表示不限" />
    </Form.Item>
  );
}

function GenerationJobDetailView({ job }: { job: AiGenerationJobDetail }) {
  const detailItems = [
    ["客户", job.customer_name || job.customer_email || "-"],
    ["任务类型", generationJobTypeLabel(job.job_type)],
    ["任务状态", generationJobStatusLabel(job.status)],
    ["模型", job.model_code ?? "-"],
    ["渠道", job.provider_name ?? "-"],
    ["三方任务", job.provider_job_id ?? "-"],
    ["三方状态", job.provider_status ?? "-"],
    ["计费", `${billingModeLabel(job.charge_mode as AiModelBillingMode)} x ${job.quantity}`],
    ["预扣", money(job.held_minor, job.currency)],
    ["扣费", money(job.charged_minor, job.currency)],
    ["退款", money(job.refunded_minor, job.currency)],
    ["尝试次数", String(job.attempts)],
    ["提交时间", job.submitted_at ? dateTime(job.submitted_at) : "-"],
    ["完成时间", job.completed_at ? dateTime(job.completed_at) : "-"],
    ["下次查询", job.next_poll_at ? dateTime(job.next_poll_at) : "-"],
    ["异常原因", job.failure_reason ?? "-"]
  ];

  return (
    <Space direction="vertical" size={16} className="settings-stack">
      <div className="settings-grid-inner">
        {detailItems.map(([label, value]) => (
          <Space key={label} direction="vertical" size={2}>
            <Typography.Text type="secondary">{label}</Typography.Text>
            <Typography.Text copyable={label === "三方任务" && value !== "-"}>{value}</Typography.Text>
          </Space>
        ))}
      </div>
      <JsonBlock title="请求参数" value={job.request_payload} />
      <JsonBlock title="三方提交返回" value={job.provider_submit_response} />
      <JsonBlock title="三方结果返回" value={job.provider_result_response} />
      <JsonBlock
        title="缓存素材"
        value={{
          result: job.result,
          asset_urls: job.asset_urls
        }}
      />
    </Space>
  );
}

function JsonBlock({ title, value }: { title: string; value: unknown }) {
  return (
    <Space direction="vertical" size={6} className="settings-stack">
      <Typography.Text strong>{title}</Typography.Text>
      <pre className="json-view">{stringifyJson(value)}</pre>
    </Space>
  );
}

function aiBillingSectionFromPath(pathname: string): AiBillingSection {
  if (pathname === "/logs/ai-usage") {
    return "usage";
  }
  if (pathname === "/logs/ai-jobs") {
    return "jobs";
  }
  if (pathname === "/logs/ai-assets") {
    return "assets";
  }
  if (pathname.endsWith("/models")) {
    return "models";
  }
  if (pathname.endsWith("/wallets")) {
    return "wallets";
  }
  if (pathname.endsWith("/usage")) {
    return "usage";
  }
  if (pathname.endsWith("/jobs")) {
    return "jobs";
  }
  if (pathname.endsWith("/assets")) {
    return "assets";
  }

  return "providers";
}

function buildProviderPayload(values: ProviderFormValues, editing: boolean) {
  const config = parseJsonObject(values.config_json);
  const secret: Record<string, unknown> = {};
  if (values.api_key?.trim()) {
    secret.api_key = values.api_key.trim();
  }

  return {
    name: values.name.trim(),
    kind: values.kind,
    base_url: values.base_url.trim(),
    enabled: values.enabled,
    config,
    ...(Object.keys(secret).length > 0 ? { secret } : {}),
    ...(editing ? {} : { kind: values.kind })
  };
}

function buildModelPayload(values: ModelFormValues) {
  const priceFields = priceFieldsForBillingMode(values);
  const pricingConfig = mergeModelCapabilities(parseJsonObject(values.pricing_config_json), values);

  return {
    name: values.name.trim(),
    modality: values.modality,
    enabled: values.enabled,
    currency: values.currency.trim().toUpperCase(),
    billing_mode: values.billing_mode,
    ...priceFields,
    daily_spend_limit_minor:
      values.daily_spend_limit == null ? null : moneyToMinor(values.daily_spend_limit),
    pricing_config: pricingConfig,
    metadata: parseJsonObject(values.metadata_json)
  };
}

function priceFieldsForBillingMode(values: ModelFormValues) {
  const zeroPrices = {
    input_1k_price_minor: 0,
    output_1k_price_minor: 0,
    request_price_minor: 0,
    image_price_minor: 0,
    second_price_minor: 0,
    minute_price_minor: 0
  };

  switch (values.billing_mode) {
    case "token":
      return {
        ...zeroPrices,
        input_1k_price_minor: moneyToMinor(values.input_1k_price),
        output_1k_price_minor: moneyToMinor(values.output_1k_price)
      };
    case "per_image":
      return {
        ...zeroPrices,
        image_price_minor: moneyToMinor(values.image_price)
      };
    case "video_per_second":
    case "audio_per_second":
      return {
        ...zeroPrices,
        second_price_minor: moneyToMinor(values.second_price)
      };
    case "video_per_request":
    case "audio_per_request":
      return {
        ...zeroPrices,
        request_price_minor: moneyToMinor(values.request_price)
      };
    case "audio_per_minute":
      return {
        ...zeroPrices,
        minute_price_minor: moneyToMinor(values.minute_price)
      };
  }

  return zeroPrices;
}

function parseJsonObject(value: string): Record<string, unknown> {
  const parsed = JSON.parse(value || "{}");
  if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) {
    throw new Error("JSON 必须是对象");
  }

  return parsed as Record<string, unknown>;
}

function defaultProviderConfigForKind(kind: AiProviderKind): Record<string, unknown> {
  if (kind === "wuyin_keji") {
    return {
      timeout_seconds: 120,
      detail_path: "/api/async/detail",
      detail_method: "GET",
      detail_id_field: "id",
      model_templates: wuyinKejiModelTemplates
    };
  }

  return {};
}

function templatesForProviderAndModality(
  provider: AiProvider | undefined,
  modality: AiModelModality
): ProviderModelTemplate[] {
  return providerModelTemplates(provider).filter((template) => template.modality === modality);
}

function findProviderModelTemplate(
  provider: AiProvider | undefined,
  modality: AiModelModality,
  providerModel: unknown
): ProviderModelTemplate | undefined {
  const value = String(providerModel ?? "")
    .trim()
    .toLowerCase();
  if (!value) {
    return undefined;
  }

  return templatesForProviderAndModality(provider, modality).find(
    (template) => template.provider_model.trim().toLowerCase() === value
  );
}

function providerModelOptionMatchesCurrentValue(inputValue: string, currentValue: unknown): boolean {
  const input = inputValue.trim().toLowerCase();
  const current = String(currentValue ?? "")
    .trim()
    .toLowerCase();

  return Boolean(input && current && input === current);
}

function shouldReplaceModelNameWithTemplate(
  provider: AiProvider | undefined,
  currentName: unknown
): boolean {
  const name = String(currentName ?? "").trim();
  if (!name) {
    return true;
  }

  return providerModelTemplates(provider).some((template) => template.name?.trim() === name);
}

function providerModelTemplates(provider: AiProvider | undefined): ProviderModelTemplate[] {
  if (!provider) {
    return [];
  }
  const configured = parseProviderModelTemplates(provider.config.model_templates);
  if (configured.length > 0) {
    return configured;
  }

  return provider.kind === "wuyin_keji" ? wuyinKejiModelTemplates : [];
}

function parseProviderModelTemplates(value: unknown): ProviderModelTemplate[] {
  if (!Array.isArray(value)) {
    return [];
  }

  return value
    .map((item) => normalizeProviderModelTemplate(item))
    .filter((item): item is ProviderModelTemplate => Boolean(item));
}

function normalizeProviderModelTemplate(value: unknown): ProviderModelTemplate | undefined {
  const source = objectValue(value);
  if (!source) {
    return undefined;
  }
  const providerModel = stringValue(source.provider_model ?? source.model);
  const modality = stringValue(source.modality ?? source.type) as AiModelModality | undefined;
  if (!providerModel || !modality || !modalityOptions.some((option) => option.value === modality)) {
    return undefined;
  }
  const billingMode = stringValue(source.billing_mode) as AiModelBillingMode | undefined;
  const pricingConfig = objectValue(source.pricing_config ?? source.config);
  const metadata = objectValue(source.metadata);

  return {
    provider_model: providerModel,
    name: stringValue(source.name),
    modality,
    ...(billingMode && billingModeLabels[billingMode] ? { billing_mode: billingMode } : {}),
    ...(pricingConfig ? { pricing_config: pricingConfig } : {}),
    ...(metadata ? { metadata } : {})
  };
}

function modelFormValuesFromTemplate(
  template: ProviderModelTemplate,
  options: { applyName?: boolean } = {}
): Partial<ModelFormValues> {
  const pricingConfig = template.pricing_config ?? {};
  const capabilities = modelCapabilities(pricingConfig);
  const nextValues: Partial<ModelFormValues> = {
    provider_model: template.provider_model,
    billing_mode: template.billing_mode ?? defaultBillingModeForModality(template.modality),
    pricing_config_json: stringifyJson(pricingConfig),
    ratios: capabilities.ratios,
    resolutions: capabilities.resolutions,
    durations: capabilities.durations.map(String),
    default_duration_seconds: capabilities.default_duration_seconds,
    image_counts: capabilities.image_counts.map(String),
    max_images: capabilities.max_images
  };
  if (options.applyName && template.name) {
    nextValues.name = template.name;
  }
  if (template.metadata && Object.keys(template.metadata).length > 0) {
    nextValues.metadata_json = stringifyJson(template.metadata);
  }

  return nextValues;
}

interface ModelCapabilityValues {
  ratios: string[];
  resolutions: string[];
  durations: number[];
  default_duration_seconds: number | null;
  image_counts: number[];
  max_images: number | null;
}

function mergeModelCapabilities(
  config: Record<string, unknown>,
  values: ModelFormValues
): Record<string, unknown> {
  const capabilities = {
    ...objectValue(config.capabilities),
    ratios: normalizeStringList(values.ratios),
    resolutions: normalizeStringList(values.resolutions),
    durations: normalizeNumberList(values.durations),
    default_duration_seconds: normalizeOptionalNumber(values.default_duration_seconds),
    image_counts: normalizeNumberList(values.image_counts),
    max_images: normalizeOptionalNumber(values.max_images)
  };
  const cleanedCapabilities = removeEmptyObjectFields(capabilities);
  const { capabilities: _unusedCapabilities, ...restConfig } = config;

  return {
    ...restConfig,
    ...(Object.keys(cleanedCapabilities).length > 0 ? { capabilities: cleanedCapabilities } : {})
  };
}

function modelCapabilities(config: Record<string, unknown>): ModelCapabilityValues {
  const source = objectValue(config.capabilities) ?? config;

  return {
    ratios: normalizeStringList(arrayValue(source.ratios ?? source.aspect_ratios)),
    resolutions: normalizeStringList(arrayValue(source.resolutions ?? source.sizes)),
    durations: normalizeNumberList(arrayValue(source.durations ?? source.duration_seconds_options)),
    default_duration_seconds: normalizeOptionalNumber(
      source.default_duration_seconds ?? source.duration_seconds ?? source.seconds
    ),
    image_counts: normalizeNumberList(arrayValue(source.image_counts ?? source.counts)),
    max_images: normalizeOptionalNumber(source.max_images ?? source.max_image_count)
  };
}

function defaultCapabilitiesForModality(modality?: AiModelModality): Partial<ModelFormValues> {
  switch (modality) {
    case "image":
      return {
        ratios: normalizeStringList(defaultImageRatios),
        resolutions: normalizeStringList(defaultImageResolutions),
        image_counts: normalizeNumberList(defaultImageCounts).map(String),
        max_images: 4,
        durations: [],
        default_duration_seconds: null
      };
    case "video":
      return {
        ratios: normalizeStringList(defaultVideoRatios),
        resolutions: normalizeStringList(defaultVideoResolutions),
        durations: normalizeNumberList(defaultVideoDurations).map(String),
        default_duration_seconds: 8,
        image_counts: [],
        max_images: null
      };
    default:
      return {
        ratios: [],
        resolutions: [],
        durations: [],
        default_duration_seconds: null,
        image_counts: [],
        max_images: null
      };
  }
}

function supportsVisualCapabilities(modality: AiModelModality): boolean {
  return modality === "image" || modality === "video" || modality === "multimodal";
}

function ratioOptionsForModality(modality: AiModelModality) {
  const values = modality === "video" ? defaultVideoRatios : defaultImageRatios;

  return values.map((value) => ({ value, label: value }));
}

function resolutionOptionsForModality(modality: AiModelModality) {
  const values = modality === "video" ? defaultVideoResolutions : defaultImageResolutions;

  return values.map((value) => ({ value, label: value }));
}

function mergedSelectOptions(...values: unknown[]): Array<{ value: string; label: string }> {
  return normalizeStringList(values.flatMap((value) => (Array.isArray(value) ? value : [value]))).map(
    (value) => ({ value, label: value })
  );
}

function normalizeStringList(value: unknown): string[] {
  const items = Array.isArray(value) ? value : [];

  return Array.from(
    new Set(
      items
        .map((item) => String(item).trim())
        .filter((item) => item.length > 0)
    )
  );
}

function normalizeNumberList(value: unknown): number[] {
  const items = Array.isArray(value) ? value : [];

  return Array.from(
    new Set(
      items
        .map((item) => Number(item))
        .filter((item) => Number.isFinite(item) && item > 0)
        .map((item) => Math.ceil(item))
    )
  ).sort((a, b) => a - b);
}

function normalizeOptionalNumber(value: unknown): number | null {
  if (value == null || value === "") {
    return null;
  }
  const number = Number(value);

  return Number.isFinite(number) && number > 0 ? Math.ceil(number) : null;
}

function objectValue(value: unknown): Record<string, unknown> | undefined {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : undefined;
}

function arrayValue(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function stringValue(value: unknown): string | undefined {
  return typeof value === "string" ? value.trim() || undefined : undefined;
}

function removeEmptyObjectFields(value: Record<string, unknown>): Record<string, unknown> {
  return Object.fromEntries(
    Object.entries(value).filter(([, item]) => {
      if (Array.isArray(item)) {
        return item.length > 0;
      }

      return item !== null && item !== undefined && item !== "";
    })
  );
}

function validateJsonField(_: unknown, value?: string) {
  try {
    parseJsonObject(value ?? "{}");
    return Promise.resolve();
  } catch {
    return Promise.reject(new Error("JSON 格式不正确"));
  }
}

function stringifyJson(value: unknown): string {
  return JSON.stringify(value ?? {}, null, 2);
}

function moneyToMinor(value?: number): number {
  return Math.round((value ?? 0) * 100);
}

function minorToMoneyNumber(value: number): number {
  return value / 100;
}

function money(value: number, currency: string): string {
  const sign = value < 0 ? "-" : "";
  const amount = Math.abs(value) / 100;

  return `${sign}${currency} ${amount.toFixed(2)}`;
}

function limitText(value: number | null | undefined, currency: string): string {
  return value == null ? "不限" : money(value, currency);
}

function isAiWalletEnabled(wallet: AiWallet): boolean {
  return wallet.ai_enabled !== false;
}

function canRetryGenerationJobPoll(job: AiGenerationJob): boolean {
  return (
    Boolean(job.provider_job_id) &&
    ["submitted", "running", "caching", "timeout_review"].includes(job.status)
  );
}

function canRetryGenerationJobCache(job: AiGenerationJob): boolean {
  return (
    job.charged_minor <= 0 &&
    ["caching", "timeout_review", "failed", "provider_failed"].includes(job.status)
  );
}

function canFailReleaseGenerationJob(job: AiGenerationJob): boolean {
  return (
    job.charged_minor <= 0 &&
    ["submitted", "running", "caching", "timeout_review", "provider_failed", "failed"].includes(
      job.status
    )
  );
}

function canRefundGenerationJob(job: AiGenerationJob): boolean {
  return job.status === "succeeded" && job.charged_minor > job.refunded_minor;
}

function formatBytes(value?: number | null): string {
  if (value == null) {
    return "-";
  }
  if (value < 1024) {
    return `${value} B`;
  }
  if (value < 1024 * 1024) {
    return `${(value / 1024).toFixed(1)} KB`;
  }

  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

function providerKindLabel(value: AiProviderKind): string {
  return providerKindOptions.find((option) => option.value === value)?.label ?? value;
}

function modalityLabel(value: AiModelModality): string {
  return modalityOptions.find((option) => option.value === value)?.label ?? value;
}

function defaultBillingModeForModality(modality: AiModelModality): AiModelBillingMode {
  switch (modality) {
    case "image":
      return "per_image";
    case "video":
      return "video_per_second";
    case "audio":
      return "audio_per_second";
    case "text":
    case "embedding":
    case "multimodal":
      return "token";
  }
}

function billingModeLabel(value: AiModelBillingMode): string {
  return billingModeLabels[value] ?? value;
}

function billingPriceSummary(record: AiModel): string {
  switch (record.billing_mode) {
    case "token":
      return `输入 ${money(record.input_1k_price_minor, record.currency)} / 1K，输出 ${money(
        record.output_1k_price_minor,
        record.currency
      )} / 1K`;
    case "per_image":
      return `每张图片 ${money(record.image_price_minor, record.currency)}`;
    case "video_per_second":
      return `每秒视频 ${money(record.second_price_minor, record.currency)}`;
    case "video_per_request":
      return `每次视频请求 ${money(record.request_price_minor, record.currency)}`;
    case "audio_per_second":
      return `每秒音频 ${money(record.second_price_minor, record.currency)}`;
    case "audio_per_minute":
      return `每分钟音频 ${money(record.minute_price_minor, record.currency)}`;
    case "audio_per_request":
      return `每次音频请求 ${money(record.request_price_minor, record.currency)}`;
  }
}

function modelCapabilitiesSummary(record: AiModel): string {
  const capabilities = modelCapabilities(record.pricing_config);
  const parts: string[] = [];
  if (capabilities.ratios.length > 0) {
    parts.push(`比例 ${capabilities.ratios.join(" / ")}`);
  }
  if (capabilities.resolutions.length > 0) {
    parts.push(`分辨率 ${capabilities.resolutions.join(" / ")}`);
  }
  if (capabilities.durations.length > 0) {
    parts.push(`时长 ${capabilities.durations.join(" / ")} 秒`);
  } else if (capabilities.default_duration_seconds) {
    parts.push(`默认 ${capabilities.default_duration_seconds} 秒`);
  }
  if (capabilities.image_counts.length > 0) {
    parts.push(`张数 ${capabilities.image_counts.join(" / ")}`);
  } else if (capabilities.max_images) {
    parts.push(`最多 ${capabilities.max_images} 张`);
  }

  return parts.length > 0 ? parts.join("；") : "未限制";
}

function ledgerTypeLabel(value: string): string {
  const labels: Record<string, string> = {
    credit: "充值",
    debit: "扣减",
    hold: "预扣",
    capture: "结算",
    release: "释放",
    refund: "退款",
    adjustment: "调整"
  };

  return labels[value] ?? value;
}

function usageStatusLabel(value: string): string {
  const labels: Record<string, string> = {
    pending: "等待中",
    running: "处理中",
    succeeded: "成功",
    failed: "失败",
    refunded: "已退款"
  };

  return labels[value] ?? value;
}

function generationJobTypeLabel(value: AiGenerationJobType): string {
  const labels: Record<AiGenerationJobType, string> = {
    image: "图片",
    video: "视频"
  };

  return labels[value] ?? value;
}

function generationJobStatusLabel(value: AiGenerationJobStatus): string {
  const labels: Record<AiGenerationJobStatus, string> = {
    pending: "待提交",
    submitted: "已提交",
    running: "生成中",
    provider_succeeded: "三方成功",
    caching: "缓存中",
    succeeded: "完成",
    provider_failed: "三方失败",
    failed: "失败",
    timeout_review: "超时待确认",
    cancelled: "已取消"
  };

  return labels[value] ?? value;
}

function assetTypeLabel(value: AiAssetType): string {
  const labels: Record<AiAssetType, string> = {
    image: "图片",
    video: "视频",
    audio: "音频",
    file: "文件"
  };

  return labels[value] ?? value;
}

function assetStatusLabel(value: AiAssetStatus): string {
  const labels: Record<AiAssetStatus, string> = {
    caching: "缓存中",
    ready: "可用",
    failed: "失败",
    deleted: "已删除"
  };

  return labels[value] ?? value;
}
