# Web 后端接入指南

本文给 Web 产品、SaaS 后端、业务服务端接入 EntitleHub AI 网关使用。浏览器端不要保存 EntitleHub Server Key，也不要直接请求第三方 AI 平台。

## 1. 接入定位

推荐长期商用架构：

```text
浏览器 / App
  -> 你的业务后端
    -> EntitleHub Server API
      -> 第三方 AI 平台
      -> EntitleHub 缓存素材
```

核心原则：

- 浏览器只登录你的业务系统，不保存 EntitleHub Server Key。
- 你的业务后端持有 EntitleHub Server Key。
- 每次调用都把实际消费的 EntitleHub 客户 ID 传给 EntitleHub。
- EntitleHub 负责订阅校验、客户状态校验、AI 钱包余额校验、预扣、扣费、失败释放、素材缓存和审计。
- 第三方平台状态以第三方查询接口返回为准，EntitleHub 不猜测任务成功或失败。

## 2. 后台准备

### 2.1 创建应用

在后台 `客户与应用 -> 应用管理` 创建或选择一个应用。Server Key 会绑定到应用。

生产建议：

- 一个 Web 产品后端使用一个应用。
- 多个产品不要共用一个应用和 Server Key。
- 测试环境和生产环境分开应用、分开 Server Key。

### 2.2 创建 Server Key

在应用详情里创建 Server Key。

要求：

- `scopes` 至少包含 `ai:invoke`。
- 明文 Key 只显示一次，创建后立即保存到你的业务后端密钥管理中。
- 不要把 Server Key 放在浏览器、移动端、前端构建产物或公开仓库里。

请求接口：

```http
POST /api/admin/server-api-keys
```

调用 AI 时使用：

```http
Authorization: Bearer ehsk_...
X-EntitleHub-Customer-Id: <customer_id>
```

### 2.3 配置 AI 渠道和模型价格

在后台 `接口计费` 下配置：

- AI 渠道：第三方平台地址、类型、密钥和公开配置。
- 模型价格：对外模型代码、三方模型名、计费方式和售价。
- 客户余额：客户 AI 钱包余额、每日限额、是否冻结 AI 权限。
- 客户订阅：客户必须有 active/trialing 且未过期订阅，AI API 才能使用。

计费方式：

- 文本：输入 / 输出 token。
- 图片：按张。
- 视频：按秒或按次。
- 音频：按秒、按分钟或按次。

## 3. 服务端同步 AI 接口

适合文本、embedding、同步图片/视频等短请求。

```http
POST /api/server/ai/v1/chat/completions
POST /api/server/ai/v1/embeddings
POST /api/server/ai/v1/images/generations
POST /api/server/ai/v1/videos/generations
GET  /api/server/ai/v1/models
Authorization: Bearer ehsk_...
X-EntitleHub-Customer-Id: uuid
Content-Type: application/json
```

示例：

```bash
curl https://entitlehub.example.com/api/server/ai/v1/chat/completions \
  -H "Authorization: Bearer ehsk_..." \
  -H "X-EntitleHub-Customer-Id: 00000000-0000-0000-0000-000000000001" \
  -H "Content-Type: application/json" \
  -H "Idempotency-Key: web-order-123-chat-1" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "hello"}],
    "max_tokens": 128
  }'
```

同步接口流程：

1. 你的业务后端鉴权当前 Web 用户。
2. 找到该 Web 用户对应的 EntitleHub `customer_id`。
3. 用 Server Key 请求 EntitleHub。
4. EntitleHub 检查应用、客户、订阅、钱包、限额。
5. EntitleHub 预扣余额。
6. EntitleHub 请求第三方平台。
7. 第三方成功则确认扣费；失败则释放预扣。
8. 你的业务后端把结果返回给浏览器。

## 4. 异步图片/视频任务

适合速创等“提交任务 -> 返回任务 ID -> 查询任务结果”的平台。

### 4.1 创建任务

图片：

```http
POST /api/server/ai/v1/images/jobs
Authorization: Bearer ehsk_...
X-EntitleHub-Customer-Id: uuid
Idempotency-Key: optional-unique-key
Content-Type: application/json
```

视频：

```http
POST /api/server/ai/v1/videos/jobs
Authorization: Bearer ehsk_...
X-EntitleHub-Customer-Id: uuid
Idempotency-Key: optional-unique-key
Content-Type: application/json
```

视频请求示例：

```json
{
  "model": "google-omni",
  "prompt": "一个 8 秒产品展示视频",
  "duration": 8,
  "size": "1280x720"
}
```

创建响应：

```json
{
  "job": {
    "id": "uuid",
    "job_type": "video",
    "status": "submitted",
    "provider_job_id": "third-party-task-id",
    "charge_mode": "video_per_second",
    "quantity": 8,
    "held_minor": 800,
    "charged_minor": 0,
    "asset_urls": [],
    "created_at": "2026-06-11T12:00:00Z"
  }
}
```

### 4.2 查询任务

```http
GET /api/server/ai/v1/jobs/{job_id}
Authorization: Bearer ehsk_...
X-EntitleHub-Customer-Id: uuid
```

任务状态：

| 状态 | 含义 | Web 后端处理 |
| --- | --- | --- |
| `submitted` | 已提交三方 | 继续轮询 |
| `running` | 三方生成中 | 继续轮询 |
| `caching` | 三方成功，EntitleHub 正在缓存素材 | 继续轮询 |
| `succeeded` | 生成成功、素材已缓存、已扣费 | 返回素材 URL |
| `provider_failed` | 三方明确失败 | 提示失败，余额已释放 |
| `failed` | 内部失败或人工失败 | 提示失败，按任务记录判断是否释放/退款 |
| `timeout_review` | 长时间未得到最终结果 | 提示处理中或联系客服 |
| `cancelled` | 已取消 | 提示取消 |

### 4.3 轮询建议

建议业务后端轮询，不建议浏览器直接轮询 EntitleHub。

```text
0-60 秒：每 3-5 秒查询一次
1-10 分钟：每 10-20 秒查询一次
10 分钟后：降低频率，或改为后台任务继续查
```

如果任务进入 `timeout_review`，不要在业务侧自行判定失败。进入后台 `任务与日志 -> 生成任务` 查看三方原始返回后人工处理。

### 4.4 素材缓存和下发

第三方成功后，EntitleHub 会下载第三方图片/视频并缓存，最终返回自己的素材 URL：

```json
{
  "asset_urls": [
    "https://entitlehub.example.com/api/ai/assets/asset-id"
  ]
}
```

业务后端和浏览器只使用 EntitleHub 返回的 URL，不要使用第三方原始 URL。

原因：

- 不暴露第三方平台地址和任务信息。
- 避免第三方临时 URL 过期导致客户无法访问。
- 方便后续做鉴权、审计、清理、迁移对象存储。

## 5. 业务后端参考流程

### 5.1 创建异步视频任务

```ts
async function createVideoJob(input: {
  entitleHubBaseUrl: string;
  serverKey: string;
  customerId: string;
  idempotencyKey: string;
  prompt: string;
  duration: number;
}) {
  const response = await fetch(`${input.entitleHubBaseUrl}/api/server/ai/v1/videos/jobs`, {
    method: "POST",
    headers: {
      "Authorization": `Bearer ${input.serverKey}`,
      "X-EntitleHub-Customer-Id": input.customerId,
      "Idempotency-Key": input.idempotencyKey,
      "Content-Type": "application/json"
    },
    body: JSON.stringify({
      model: "google-omni",
      prompt: input.prompt,
      duration: input.duration,
      size: "1280x720"
    })
  });

  const body = await response.json();
  if (!response.ok || body.code !== 0) {
    throw new Error(body.message ?? "create video job failed");
  }

  return body.data.job;
}
```

### 5.2 查询任务

```ts
async function getGenerationJob(input: {
  entitleHubBaseUrl: string;
  serverKey: string;
  customerId: string;
  jobId: string;
}) {
  const response = await fetch(`${input.entitleHubBaseUrl}/api/server/ai/v1/jobs/${input.jobId}`, {
    headers: {
      "Authorization": `Bearer ${input.serverKey}`,
      "X-EntitleHub-Customer-Id": input.customerId
    }
  });

  const body = await response.json();
  if (!response.ok || body.code !== 0) {
    throw new Error(body.message ?? "query generation job failed");
  }

  return body.data.job;
}
```

## 6. 错误处理

常见错误：

| 场景 | 业务侧处理 |
| --- | --- |
| Server Key 无效、过期、吊销 | 服务端报警，检查后台 Server Key |
| 客户无订阅或订阅过期 | 允许登录，但禁用 AI 功能，引导开通/续费 |
| 客户 AI 钱包余额不足 | 引导充值 |
| 客户 AI 权限被冻结 | 提示账号 AI 功能不可用 |
| 日限额超出 | 提示今日额度已用完 |
| 三方失败 | 展示失败，余额一般已释放 |
| `timeout_review` | 展示处理中，并通知后台人工查看 |

业务后端必须记录：

- 本地用户 ID。
- EntitleHub `customer_id`。
- EntitleHub `job.id` 或 usage id。
- 本次业务请求 ID。
- 幂等键。

## 7. 幂等和防盗刷

建议每次用户发起生成请求时生成稳定幂等键：

```text
<product>-<user_id>-<business_order_id>-<operation>
```

规则：

- 同一个业务订单重复提交同一个幂等键。
- 不同用户、不同任务、不同按钮点击不要共用幂等键。
- 用户刷新页面或前端重试时，业务后端应复用原任务 ID，而不是新建任务。
- Server Key 只放业务后端，不能下发给浏览器。
- 业务后端要对自己的用户做登录态、频率、额度和业务权限校验。

EntitleHub 负责平台级校验和钱包扣费，但你的业务后端仍要防止自己的业务入口被刷。

## 8. 后台任务详情和人工处理

后台位置：

```text
任务与日志 -> 生成任务
```

### 8.1 任务详情

点击 `查看` 可以看到：

- 客户、任务类型、任务状态。
- 模型和渠道。
- 三方任务 ID、三方状态。
- 计费方式、数量、预扣、扣费、退款金额。
- 提交时间、完成时间、下次查询时间。
- 异常原因。
- `request_payload`：客户请求参数。
- `provider_submit_response`：提交三方任务时的原始响应。
- `provider_result_response`：查询三方任务结果时的原始响应。
- 缓存后的素材 URL。

用途：

- 判断三方是否真的成功/失败。
- 查三方返回里有没有素材 URL。
- 排查为什么任务进入 `timeout_review` 或 `caching`。
- 给客服定位客户问题。

### 8.2 人工处理动作

| 动作 | 适用状态 | 结果 |
| --- | --- | --- |
| 重新查询 | `submitted`、`running`、`caching`、`timeout_review` | 任务重新进入查询队列，最终仍以三方返回为准 |
| 重新缓存 | `caching`、`timeout_review`、`failed`、`provider_failed` 且未扣费 | 使用已保存的三方结果重新下载并缓存素材，成功后扣费 |
| 标记失败 | 未扣费任务 | 释放预扣金额，任务标记为失败 |
| 人工退款 | 已成功扣费任务 | 退回已扣金额，写入退款流水和审计日志 |

注意：

- `重新缓存` 不允许对已扣费任务重复执行，避免重复扣费。
- `标记失败` 不用于已扣费任务，已扣费任务走 `人工退款`。
- 所有人工动作都会写入审计日志。
- Viewer 只能查看，Owner/Admin/Developer 可以处理。
- 后台处理不会修改第三方平台状态，只修改 EntitleHub 内部商业状态。

### 8.3 什么时候需要人工处理

需要人工处理的典型情况：

- 三方查询接口长时间没有最终结果，任务进入 `timeout_review`。
- 三方返回成功，但素材下载失败，任务卡在 `caching`。
- 三方文档变更导致素材 URL 字段没被正确识别。
- 客户投诉已经扣费但结果不可用，需要人工退款。
- 三方返回失败但原因需要客服解释。

处理建议：

1. 先打开任务详情，看 `provider_result_response`。
2. 三方还在运行：点 `重新查询`。
3. 三方已成功且有素材 URL：点 `重新缓存`。
4. 三方确认失败且未扣费：点 `标记失败`。
5. 已扣费但客户不可用：点 `人工退款`。

## 9. 生产上线检查

上线前确认：

- Server Key 已保存到业务后端密钥管理，不在前端。
- 业务后端所有请求都传 `X-EntitleHub-Customer-Id`。
- 客户订阅、钱包余额、AI 权限冻结流程已在后台验证。
- 图片/视频异步任务使用 `/images/jobs`、`/videos/jobs`。
- 业务后端保存 EntitleHub `job.id`，不要只保存三方任务 ID。
- 任务轮询有退避策略，不要高频打满。
- 客户可见素材 URL 使用 EntitleHub 返回的 `asset_urls`。
- `timeout_review` 有后台人工处理流程。
- 客服能在 `任务与日志 -> 生成任务` 查到任务详情。
- 生产日志不要打印 Server Key、第三方密钥、客户隐私请求体。
