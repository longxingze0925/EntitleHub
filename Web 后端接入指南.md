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

### 2.3 配置 AI 渠道和模型商品

在后台 `接口计费` 下配置：

- AI 渠道：第三方平台地址、类型、密钥和公开配置。
- 模型商品：对外模型代码、三方模型名、可用比例、分辨率、时长、张数、计费方式和售价。
- 客户余额：客户 AI 钱包余额、每日限额、是否冻结 AI 权限。
- 客户订阅：客户必须有 active/trialing 且未过期订阅，AI API 才能使用。

计费方式：

- 文本：输入 / 输出 token。
- 图片：按张。
- 视频：按秒或按次。
- 音频：按秒、按分钟或按次。

模型商品原则：

- Web 产品只使用 EntitleHub 的模型代码，不直接使用第三方真实模型名。
- 图片模型可配置允许比例、分辨率、单次张数和最大张数。
- 视频模型可配置允许比例、分辨率、允许时长和默认时长。
- 影织这类 Web 产品应该先调用模型列表接口，再按返回能力渲染前端选项。
- EntitleHub 会在服务端强校验比例、分辨率、时长、张数，不能只靠前端限制。

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

### 3.1 获取可用模型商品

影织后端启动时或定时刷新时调用：

```http
GET /api/server/ai/v1/models
Authorization: Bearer ehsk_...
X-EntitleHub-Customer-Id: uuid
```

响应中的 `data` 会返回可用模型商品：

```json
{
  "object": "list",
  "data": [
    {
      "id": "yingzhi-video-fast",
      "object": "model",
      "name": "影织快速视频",
      "modality": "video",
      "provider_model": "google_omni",
      "billing": {
        "currency": "CNY",
        "mode": "video_per_second",
        "second_price_minor": 20,
        "request_price_minor": 0
      },
      "capabilities": {
        "ratios": [],
        "resolutions": ["1280x720", "720x1280", "1920x1080", "1080x1920"],
        "durations": [10],
        "default_duration_seconds": 10,
        "image_counts": [],
        "max_images": null,
        "inputModes": ["text", "image", "frames", "video"],
        "maxReferenceImages": 7,
        "supportsReferenceVideo": true,
        "supportsFirstFrame": true,
        "supportsLastFrame": true,
        "acceptedMimeTypes": ["image/png", "image/jpeg", "image/webp", "video/mp4"],
        "maxAssetSizeMb": 50
      }
    }
  ]
}
```

影织前端应该只展示 `capabilities` 里允许的选项。提交任务时只传 `id` 里的模型代码，例如 `yingzhi-video-fast`。

能力字段含义：

- `ratios`：支持的画面比例，例如 `16:9`、`9:16`、`1:1`。
- `resolutions`：支持的分辨率，例如 `720p`、`1080p`、`1024x1024`。
- `durations`：视频可选时长，单位秒。
- `image_counts` / `max_images`：图片生成可选张数和最大张数。
- `inputModes`：允许的输入方式，常用 `text`、`image`、`frames`、`video`。
- `maxReferenceImages`：最多可传多少个参考素材。
- `supportsReferenceVideo`：是否允许把视频作为参考素材。
- `supportsFirstFrame` / `supportsLastFrame`：是否支持首帧、尾帧。
- `acceptedMimeTypes`：参考素材允许的 MIME 类型。
- `maxAssetSizeMb`：单个参考素材最大体积。

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

## 3A. Web 产品平台 API

影织这类 Web 产品建议优先使用这一组接口。它们仍然只允许业务后端调用，浏览器不要直接请求 EntitleHub。

通用请求头：

```http
Authorization: Bearer ehsk_...
Content-Type: application/json
```

### 3A.1 客户注册

```http
POST /api/server/web/v1/customers/register
```

请求：

```json
{
  "email": "user@example.com",
  "password": "Strong@12345",
  "name": "用户昵称"
}
```

响应：

```json
{
  "user": {
    "id": "uuid",
    "email": "user@example.com",
    "name": "用户昵称",
    "customerId": "uuid",
    "status": "active",
    "email_verified": false
  }
}
```

### 3A.2 客户登录

```http
POST /api/server/web/v1/customers/login
```

请求：

```json
{
  "email": "user@example.com",
  "password": "Strong@12345"
}
```

说明：

- EntitleHub 只校验客户账号密码并返回客户身份。
- 影织自己的登录态、Cookie、JWT、Session 仍由影织后端自己维护。
- 不返回 EntitleHub access token，不走 `/api/client/auth/*` 设备授权体系。

### 3A.3 客户信息、余额、用量、套餐

```http
GET /api/server/web/v1/customers/{customerId}
GET /api/server/web/v1/customers/{customerId}/balance
GET /api/server/web/v1/customers/{customerId}/usage?page=1&page_size=20
GET /api/server/web/v1/customers/{customerId}/plan
```

用途：

- `customers/{customerId}`：显示当前客户基础信息。
- `balance`：显示 AI 钱包余额、预扣金额、可用余额、AI 权限是否冻结。
- `usage`：显示客户 AI 调用和扣费记录。
- `plan`：显示当前应用下客户有效订阅；没有订阅时返回 `plan: null`。

### 3A.4 Web 模型商品

```http
GET /api/server/web/v1/ai/models
```

这是 `/api/server/ai/v1/models` 的 Web 友好别名，返回结构一致。影织前端选项应该来自返回的 `capabilities`，不要写死比例、分辨率、时长和张数。

### 3A.5 统一创建生成任务

```http
POST /api/server/web/v1/ai/jobs
Idempotency-Key: optional-unique-key
```

图片请求：

```json
{
  "customer_id": "uuid",
  "type": "image",
  "model": "yingzhi-image-fast",
  "prompt": "一张产品海报",
  "ratio": "1:1",
  "size": "1024x1024",
  "n": 1
}
```

视频请求：

```json
{
  "customer_id": "uuid",
  "type": "video",
  "model": "yingzhi-video-fast",
  "prompt": "一个 8 秒产品展示视频",
  "ratio": "16:9",
  "resolution": "1080p",
  "duration": 8
}
```

带参考素材的视频请求：

```json
{
  "customer_id": "uuid",
  "type": "video",
  "model": "yingzhi-video-fast",
  "prompt": "根据首尾帧生成一个产品展示视频",
  "inputMode": "frames",
  "referenceAssetIds": ["uuid-reference-asset"],
  "firstFrameAssetId": "uuid-first-frame",
  "lastFrameAssetId": "uuid-last-frame",
  "aspectRatio": "16:9",
  "resolution": "1080p",
  "durationSec": 8
}
```

推荐的新结构也可以直接传 `referenceAssets`，便于影织按素材角色统一组装：

```json
{
  "customer_id": "uuid",
  "type": "video",
  "model": "yingzhi-video-fast",
  "prompt": "根据首尾帧生成一个产品展示视频",
  "inputMode": "frames",
  "referenceAssets": [
    {
      "assetId": "uuid-reference-image",
      "kind": "image",
      "role": "reference"
    },
    {
      "assetId": "uuid-first-frame",
      "kind": "image",
      "role": "first_frame"
    },
    {
      "assetId": "uuid-last-frame",
      "kind": "image",
      "role": "last_frame"
    }
  ],
  "ratio": "16:9",
  "resolution": "1080p",
  "duration": 8
}
```

注意：

- `type` 只支持 `image`、`video`。
- `model` 使用 EntitleHub 模型代码，不是第三方真实模型名。
- `customer_id` 是本次扣费、订阅校验和任务归属的 EntitleHub 客户 ID。
- EntitleHub 会按模型商品配置校验参数并预扣余额。
- 参考素材字段只支持视频任务；图片任务传参考素材会被拒绝。
- `aspectRatio` 会兼容映射为 `ratio`，`durationSec` 会兼容映射为 `duration`。
- `referenceAssetIds`、`firstFrameAssetId`、`lastFrameAssetId` 和 `referenceAssets[].assetId` 必须是当前客户资产库里的 `ready` 素材。
- `referenceAssets[].role` 支持 `reference`、`first_frame`、`last_frame`；首帧/尾帧必须是图片。
- EntitleHub 会按模型 `capabilities` 校验输入方式、参考素材数量、MIME 类型、大小、是否支持首帧/尾帧。
- 如果模型不支持对应能力，会返回清晰错误，例如 `model_not_support_reference_video`、`model_not_support_first_frame`、`model_not_support_last_frame`、`reference_asset_kind_mismatch`。
- 请求会记录 `sourceMode`、`referenceCount`、`hasFirstFrame`、`hasLastFrame`，生成成功后会同步写入作品元数据。

### 3A.6 查询、列表、取消、重试任务

```http
GET  /api/server/web/v1/ai/jobs?customer_id={customerId}&type=image|video&status=running&page=1&page_size=20
GET  /api/server/web/v1/ai/jobs/{jobId}?customer_id={customerId}
POST /api/server/web/v1/ai/jobs/{jobId}/cancel
POST /api/server/web/v1/ai/jobs/{jobId}/retry
```

取消请求：

```json
{
  "customer_id": "uuid",
  "reason": "用户主动取消"
}
```

重试请求：

```json
{
  "customer_id": "uuid",
  "reason": "用户刷新任务状态"
}
```

说明：

- `cancel` 是 EntitleHub 本地取消：停止继续轮询并释放未扣费预扣金额。
- 如果第三方平台没有取消接口，EntitleHub 不能保证第三方侧也停止生成。
- `retry` 是重新查询第三方任务，不是重新创建一个新任务。
- 如果要重新生成，影织应该重新调用 `POST /api/server/web/v1/ai/jobs`，并使用新的幂等键。
- 任务返回会带 `sourceMode`、`referenceCount`、`hasFirstFrame`、`hasLastFrame`、`visibility`、`publishedAt`、`favoritedAt`、`downloadedAt`，用于影织直接渲染“我的作品”和生成历史状态。

任务列表完整响应外壳：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "items": [],
    "jobs": [],
    "meta": {
      "page": 1,
      "page_size": 20,
      "pageSize": 20,
      "hasMore": false
    },
    "pagination": {
      "page": 1,
      "page_size": 20,
      "pageSize": 20,
      "hasMore": false
    }
  },
  "request_id": "req_xxx"
}
```

任务详情完整结构：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "job": {
      "id": "uuid",
      "customer_id": "uuid",
      "model_code": "yingzhi-video-fast",
      "job_type": "video",
      "status": "succeeded",
      "progress": 100,
      "provider_status": "2",
      "provider_job_id": "third-party-task-id",
      "provider_request_id": null,
      "result": {},
      "results": [{}],
      "asset_urls": [
        "https://entitlehub.example.com/api/ai/assets/uuid"
      ],
      "assetUrls": [
        "https://entitlehub.example.com/api/ai/assets/uuid"
      ],
      "assets": [
        {
          "id": "uuid",
          "name": "result.mp4",
          "kind": "video",
          "asset_type": "video",
          "status": "ready",
          "url": "https://entitlehub.example.com/api/server/web/v1/assets/uuid/download",
          "public_url": "https://entitlehub.example.com/api/server/web/v1/assets/uuid/download",
          "mimeType": "video/mp4",
          "mime_type": "video/mp4",
          "thumbnailUrl": "https://cdn.example.com/result-cover.jpg",
          "durationSec": 8,
          "durationSeconds": 8,
          "source": "ai",
          "sourceAlias": "ai",
          "createdAt": "2026-06-14T12:00:00Z"
        }
      ],
      "workId": "uuid",
      "charge_mode": "video_per_second",
      "quantity": 8,
      "held_minor": 800,
      "charged_minor": 800,
      "refunded_minor": 0,
      "currency": "CNY",
      "failure_reason": null,
      "sourceMode": "frames",
      "referenceCount": 1,
      "hasFirstFrame": true,
      "hasLastFrame": true,
      "visibility": "private",
      "publishedAt": null,
      "favoritedAt": null,
      "downloadedAt": null,
      "created_at": "2026-06-14T12:00:00Z",
      "updated_at": "2026-06-14T12:01:00Z",
      "completed_at": "2026-06-14T12:01:00Z"
    }
  },
  "request_id": "req_xxx"
}
```

### 3A.7 资产库和文件夹

资产库用于管理 Web 用户上传素材和 AI 生成结果。典型素材包括：

- 用户上传素材。
- 参考图。
- 首帧、尾帧。
- 品牌素材。
- AI 生成后的图片、视频、音频或文件。

接口：

```http
GET    /api/server/web/v1/asset-folders?customer_id={customerId}
POST   /api/server/web/v1/asset-folders
PATCH  /api/server/web/v1/asset-folders/{folderId}
DELETE /api/server/web/v1/asset-folders/{folderId}

POST   /api/server/web/v1/assets/upload-url
POST   /api/server/web/v1/assets/upload
PUT    /api/server/web/v1/assets/uploads/{uploadId}
GET    /api/server/web/v1/assets?customer_id={customerId}
GET    /api/server/web/v1/assets/{assetId}
PATCH  /api/server/web/v1/assets/{assetId}
DELETE /api/server/web/v1/assets/{assetId}
GET    /api/server/web/v1/assets/{assetId}/download
```

所有管理接口都需要：

```http
Authorization: Bearer ehsk_...
```

文件夹创建示例：

```json
{
  "customer_id": "uuid",
  "parent_id": null,
  "name": "品牌素材",
  "metadata": {
    "scene": "brand"
  }
}
```

资产列表查询：

```http
GET /api/server/web/v1/assets?customer_id=uuid&asset_type=image&asset_role=reference&page=1&page_size=20
GET /api/server/web/v1/assets?customer_id=uuid&kind=video&source=ai&page=1&page_size=20
```

过滤规则：

- `folder_id` 不传：查询该客户全部资产。
- `folder_id=root`：只查根目录资产。
- `folder_id={uuid}`：只查指定文件夹。
- `asset_type` / `kind`：`image`、`video`、`audio`、`file`，两个字段等价，推荐 Web 产品用 `kind`。
- `asset_role`：`upload`、`generated`、`reference`、`first_frame`、`last_frame`、`brand`、`other`。
- `source`：兼容 `user_upload`、`generated`、`imported`；也支持 Web 友好别名 `upload`、`ai`、`digital-human`、`product`。

资产列表和详情的统一字段：

```json
{
  "id": "uuid",
  "name": "result.mp4",
  "kind": "video",
  "asset_type": "video",
  "status": "ready",
  "mimeType": "video/mp4",
  "url": "https://entitlehub.example.com/api/server/web/v1/assets/uuid/download",
  "thumbnailUrl": "https://cdn.example.com/result-cover.jpg",
  "duration": 8,
  "durationSec": 8,
  "durationSeconds": 8,
  "source": "generated",
  "sourceAlias": "ai",
  "createdAt": "2026-06-14T12:00:00Z"
}
```

资产列表完整响应外壳：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "items": [],
    "assets": [],
    "meta": {
      "page": 1,
      "page_size": 20,
      "pageSize": 20,
      "hasMore": false
    },
    "pagination": {
      "page": 1,
      "page_size": 20,
      "pageSize": 20,
      "hasMore": false
    }
  },
  "request_id": "req_xxx"
}
```

封面和时长说明：

- 图片资产的 `thumbnailUrl` 默认就是图片自己的 `url`。
- 视频资产优先读取资产 `metadata.thumbnailUrl` / `thumbnail_url` / `coverUrl` / `cover_url` / `posterUrl` / `poster_url`。
- 视频时长优先读取资产 `metadata.durationSec` / `duration` / `durationSeconds` / `duration_seconds` / `seconds`。
- AI 生成成功后，如果第三方结果里带封面或时长，EntitleHub 会写入资产 metadata 并在资产列表返回。
- 当前不会在同步上传请求里现场抽帧；真正的视频抽帧封面和精确时长建议后续接异步媒体处理器。

删除说明：

- 删除文件夹要求文件夹为空。
- 删除资产是软删除，不会立刻物理删除对象存储文件。
- AI 生成结果会自动写入资产库，`asset_role=generated`、`source=generated`。

### 3A.8 上传素材流程

推荐流程：

```text
浏览器 -> 影织后端：申请上传
影织后端 -> EntitleHub：创建上传会话
影织后端 -> 浏览器：返回 upload_id、url、upload_token
浏览器 -> EntitleHub：PUT 上传文件字节
EntitleHub -> 浏览器：返回 asset
```

创建上传会话：

```http
POST /api/server/web/v1/assets/upload-url
Authorization: Bearer ehsk_...
Content-Type: application/json
```

请求：

```json
{
  "customer_id": "uuid",
  "folder_id": null,
  "file_name": "reference.png",
  "kind": "image",
  "asset_role": "reference",
  "mime_type": "image/png",
  "file_size": 123456,
  "metadata": {
    "local_asset_id": "yingzhi-asset-1"
  }
}
```

响应：

```json
{
  "upload": {
    "upload_id": "uuid",
    "method": "PUT",
    "url": "https://entitlehub.example.com/api/server/web/v1/assets/uploads/{upload_id}",
    "upload_token": "ehup_...",
    "token_prefix": "ehup_xxx",
    "expires_at": "2026-06-12T12:00:00Z",
    "max_bytes": 536870912,
    "headers": {
      "X-EntitleHub-Upload-Token": "ehup_...",
      "Content-Type": "application/octet-stream"
    }
  }
}
```

上传文件：

```http
PUT /api/server/web/v1/assets/uploads/{upload_id}
X-EntitleHub-Upload-Token: ehup_...
Content-Type: image/png

<binary>
```

说明：

- 上传令牌默认 15 分钟有效。
- 上传令牌只能使用一次。
- 上传令牌只授权这一个上传会话，不能访问其他资产。
- 生产建议把上传令牌放请求头，不要放 URL 查询参数，避免被网关日志记录。
- 浏览器可以直接上传到 EntitleHub，但 Server Key 仍然只保存在影织后端。

上传成功返回会同时给完整资产对象和影织常用别名：

```json
{
  "asset": {
    "id": "uuid",
    "kind": "image",
    "asset_type": "image",
    "asset_role": "reference",
    "public_url": "https://entitlehub.example.com/api/server/web/v1/assets/uuid/download",
    "url": "https://entitlehub.example.com/api/server/web/v1/assets/uuid/download",
    "mime_type": "image/png",
    "mimeType": "image/png",
    "thumbnailUrl": "https://entitlehub.example.com/api/server/web/v1/assets/uuid/download",
    "file_size": 123456
  },
  "assetId": "uuid",
  "url": "https://entitlehub.example.com/api/server/web/v1/assets/uuid/download",
  "type": "image",
  "kind": "image",
  "mimeType": "image/png"
}
```

服务端直传小文件：

```http
POST /api/server/web/v1/assets/upload?customer_id=uuid&file_name=reference.png&kind=image&asset_role=reference&mime_type=image/png
Authorization: Bearer ehsk_...
Content-Type: image/png

<binary>
```

直传适合影织后端代传小文件。大文件仍建议两段式上传，避免业务后端中转占用内存和带宽。

第三方引用素材注意：

- EntitleHub 会按渠道适配参考素材字段。速创 `google_omni` 会把参考图、首帧、尾帧合并成三方要求的 `images`，并把 `resolution` 转成 `size`。
- 三方平台必须能访问这些 URL。生产环境建议使用可外网访问的对象存储公开 URL 或短期签名 URL。
- 如果使用本地文件存储，`public_url` 默认是 EntitleHub 下载接口，下载接口需要 Server Key，很多第三方平台无法直接拉取。
- 长期商用建议把素材存储切到 S3/R2/OSS/COS 这类对象存储，并确保三方平台可访问参考素材。

### 3A.9 作品、收藏和灵感广场

作品体系用于支撑影织这类 Web 产品的“我的作品、我的收藏、灵感广场、详情页”。

关系说明：

- `Asset`：图片、视频等文件本身。
- `Work`：用户作品，指向一个主资产，可有封面、标题、描述和业务 metadata。
- `Favorite`：某个客户收藏某个作品的关系。
- `Publication`：作品是否发布到灵感广场。

生成任务成功后，EntitleHub 会自动：

1. 下载第三方图片或视频。
2. 缓存为 EntitleHub 自己的资产 URL。
3. 写入客户资产库，`asset_role=generated`、`source=generated`。
4. 自动创建 `Work`。
5. `Work.visibility` 默认为 `private`。

作品接口：

```http
GET    /api/server/web/v1/works?customer_id={customerId}
GET    /api/server/web/v1/works?customer_id={customerId}&favorite=true
GET    /api/server/web/v1/works/{workId}?customer_id={customerId}
PATCH  /api/server/web/v1/works/{workId}
DELETE /api/server/web/v1/works/{workId}
POST   /api/server/web/v1/works/{workId}/favorite
DELETE /api/server/web/v1/works/{workId}/favorite
POST   /api/server/web/v1/works/{workId}/download
POST   /api/server/web/v1/works/{workId}/publish
POST   /api/server/web/v1/works/{workId}/unpublish
GET    /api/server/web/v1/gallery
```

作品列表支持：

```http
GET /api/server/web/v1/works?customer_id=uuid&type=video&visibility=private&page=1&page_size=20
```

返回字段核心含义：

```json
{
  "work": {
    "id": "uuid",
    "owner_customer_id": "uuid",
    "source_job_id": "uuid",
    "title": "AI 生成视频",
    "description": null,
    "work_type": "video",
    "visibility": "private",
    "primary_asset_id": "uuid",
    "primary_asset_url": "https://entitlehub.example.com/api/ai/assets/...",
    "cover_asset_id": null,
    "cover_asset_url": null,
    "favorite_count": 0,
    "favorited": false,
    "sourceMode": "frames",
    "referenceCount": 2,
    "hasFirstFrame": true,
    "hasLastFrame": true,
    "publication_status": null,
    "published_at": null,
    "publishedAt": null,
    "favoritedAt": null,
    "downloadedAt": null,
    "publication_tags": [],
    "metadata": {
      "source": "ai_generation_job",
      "job_id": "uuid",
      "sourceMode": "frames",
      "referenceCount": 2,
      "hasFirstFrame": true,
      "hasLastFrame": true
    }
  }
}
```

更新作品：

```json
{
  "customer_id": "uuid",
  "title": "我的视频作品",
  "description": "用于首页展示",
  "cover_asset_id": "uuid",
  "metadata": {
    "local_work_id": "yingzhi-work-1"
  }
}
```

收藏作品：

```json
{
  "customer_id": "uuid"
}
```

发布到灵感广场：

```json
{
  "customer_id": "uuid",
  "tags": ["产品展示", "科技感"]
}
```

取消发布：

```json
{
  "customer_id": "uuid"
}
```

标记下载并获取下载地址：

```http
POST /api/server/web/v1/works/{workId}/download
Authorization: Bearer ehsk_...
Content-Type: application/json
```

```json
{
  "customer_id": "uuid"
}
```

响应：

```json
{
  "downloadUrl": "https://entitlehub.example.com/api/ai/assets/asset-id",
  "downloadedAt": "2026-06-13T12:00:00Z",
  "work": {
    "id": "uuid",
    "downloadedAt": "2026-06-13T12:00:00Z"
  }
}
```

灵感广场：

```http
GET /api/server/web/v1/gallery?type=video&customer_id=uuid&page=1&page_size=20
```

说明：

- `customer_id` 可选；传入后返回值里的 `favorited` 会按当前客户计算。
- 灵感广场只返回当前 Server Key 绑定应用下已发布的作品。
- 删除作品是软删除，会自动从广场下架，但不会删除底层资产文件。
- 发布、取消发布、删除作品只能由作品 owner 操作。
- 收藏是客户维度关系，不是资产或作品的全局标签。
- 下载状态也是客户维度关系，EntitleHub 会记录 `downloadedAt` 和下载次数，换设备后仍能展示。

作品列表和画廊列表完整响应外壳：

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "items": [],
    "works": [],
    "meta": {
      "page": 1,
      "page_size": 20,
      "pageSize": 20,
      "hasMore": false
    },
    "pagination": {
      "page": 1,
      "page_size": 20,
      "pageSize": 20,
      "hasMore": false
    }
  },
  "request_id": "req_xxx"
}
```

## 4. 异步图片/视频任务

这一节是 EntitleHub 低层 Server AI 接口，保留给非 Web 产品直接接入。影织这类 Web 产品优先使用 3A 的统一接口：`POST /api/server/web/v1/ai/jobs`、`GET /api/server/web/v1/ai/jobs`、`GET /api/server/web/v1/ai/jobs/{jobId}`。

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
  "ratio": "16:9",
  "resolution": "1080p"
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
  const response = await fetch(`${input.entitleHubBaseUrl}/api/server/web/v1/ai/jobs`, {
    method: "POST",
    headers: {
      "Authorization": `Bearer ${input.serverKey}`,
      "Idempotency-Key": input.idempotencyKey,
      "Content-Type": "application/json"
    },
    body: JSON.stringify({
      customer_id: input.customerId,
      type: "video",
      model: "google_omni",
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
  const response = await fetch(`${input.entitleHubBaseUrl}/api/server/web/v1/ai/jobs/${input.jobId}?customer_id=${input.customerId}`, {
    headers: {
      "Authorization": `Bearer ${input.serverKey}`
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

错误响应外壳固定如下：

```json
{
  "code": 40001,
  "errorCode": "model_not_support_reference_video",
  "message": "validation_failed",
  "data": null,
  "request_id": "req_xxx"
}
```

说明：

- `code` 是 EntitleHub 兼容旧接口的数字错误码。
- `message` 是通用错误分类。
- `errorCode` 是 Web 产品更适合判断的稳定字符串；如果没有更细原因，则和 `message` 一致。

常见错误：

| 场景 | 业务侧处理 |
| --- | --- |
| Server Key 无效、过期、吊销 | 服务端报警，检查后台 Server Key |
| 客户无订阅或订阅过期 | 允许登录，但禁用 AI 功能，引导开通/续费 |
| 客户 AI 钱包余额不足 | 引导充值 |
| 客户 AI 权限被冻结 | 提示账号 AI 功能不可用 |
| 日限额超出 | 提示今日额度已用完 |
| 比例、分辨率、时长、张数不在模型能力范围内 | 重新拉取模型列表，按后台允许选项提交 |
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
- 影织这类 Web 产品统一使用 `/api/server/web/v1/ai/jobs`，客户 ID 放请求体或查询参数。
- 客户订阅、钱包余额、AI 权限冻结流程已在后台验证。
- 图片/视频前端选项来自 `/api/server/web/v1/ai/models` 的 `capabilities`，不要在影织写死。
- 业务后端保存 EntitleHub `job.id`，不要只保存三方任务 ID。
- 任务轮询有退避策略，不要高频打满。
- 客户可见素材 URL 使用 EntitleHub 返回的 `asset_urls`。
- Web 产品也可以读取任务返回的 `assetUrls`、`assets[].url`、`assets[].thumbnailUrl`、`assets[].durationSec`。
- `timeout_review` 有后台人工处理流程。
- 客服能在 `任务与日志 -> 生成任务` 查到任务详情。
- 生产日志不要打印 Server Key、第三方密钥、客户隐私请求体。
