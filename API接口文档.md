# API 接口设计文档

本文档定义后台系统第一版 API。业务接口均以 `/api` 为前缀。公开 JWKS 使用标准路径 `/.well-known/jwks.json`。

接口设计目标：

- 管理后台可完整管理租户、用户、客户、应用、授权、设备和分发内容。
- 客户端 SDK 可安全激活、登录、刷新 session、心跳、下载更新。
- 所有接口有明确认证方式、权限要求和安全注意事项。

## 1. 通用约定

### 1.1 返回格式

成功：

```json
{
  "code": 0,
  "message": "ok",
  "data": {},
  "request_id": "req_xxx"
}
```

失败：

```json
{
  "code": 40100,
  "message": "unauthenticated",
  "data": null,
  "request_id": "req_xxx"
}
```

### 1.2 错误码区间

```text
40000 参数错误
40100 认证错误
40300 权限错误
40400 资源不存在
40900 状态冲突
42900 限流
50000 服务异常
```

稳定错误码以 `权限点与错误码清单.md` 为准。代码实现不能随意新增临时错误码，确实需要新增时必须先更新该清单。

### 1.3 认证方式

后台接口：

```text
HttpOnly Cookie session
CSRF token
```

后台登录和刷新会下发可被前端读取的 `admin_csrf` Cookie。所有后台非安全方法请求（`POST`、`PUT`、`PATCH`、`DELETE`）必须同时携带：

```text
Cookie: admin_csrf=<token>
X-CSRF-Token: <token>
```

服务端会校验 Cookie 与 Header 一致，并验证 token 签名。`GET`、`HEAD`、`OPTIONS` 不要求 CSRF。

跨域部署时，浏览器请求的 `Origin` 必须出现在后端 `ALLOWED_ORIGINS` 配置中。服务端只会对明确允许的来源返回 `Access-Control-Allow-Origin`，并且不会使用 `*` 搭配 credentials。

客户端接口：

```text
Authorization: Bearer <client_access_token>
X-Device-Id
X-Device-Key-Id
X-Timestamp
X-Nonce
X-Body-SHA256
X-Signature
```

MVP 阶段至少所有客户端写接口、下载 token 申请、脚本拉取必须启用设备签名。生产上线前，所有客户端保护接口都必须启用设备签名和 nonce 防重放。

`X-Nonce` 必须是 16–128 个 URL-safe 字符：`A-Z`、`a-z`、`0-9`、`-`、`_`。

### 1.4 公开 JWKS

服务端必须提供公开 JWKS，用于客户端或第三方验证服务端签名。

```http
GET /.well-known/jwks.json
```

返回：

```json
{
  "keys": [
    {
      "kid": "jwt-2026-06-01",
      "kty": "OKP",
      "crv": "Ed25519",
      "alg": "EdDSA",
      "use": "sig",
      "x": "base64url-public-key"
    }
  ]
}
```

要求：

- 只返回 public key。
- `/.well-known/jwks.json` 返回全局 `jwt_access_token` 公钥。
- 每个 key 必须有 `kid`。
- `active` 和 `retiring` 状态的 key 可以返回。
- `revoked` key 不能用于新签名。

### 1.4.1 平台探针

平台探针不属于业务 API，不使用 `/api` 前缀。

```http
GET /health
GET /healthz
GET /readyz
GET /metrics
```

用途：

- `/health` 和 `/healthz` 用于进程存活检查。
- `/readyz` 用于依赖就绪检查。
- `/metrics` 输出 Prometheus text 格式的 HTTP、依赖、worker、通知投递和 AI 网关指标。
- AI 网关指标包括 `ai_gateway_requests_total`、`ai_gateway_charged_minor_total`、`ai_gateway_provider_duration_seconds`、`ai_gateway_asset_cache_failures_total` 和 `ai_gateway_idempotency_replays_total`。

### 1.5 分页参数

```text
page: 默认 1
page_size: 默认 20，最大 100
```

分页返回：

```json
{
  "items": [],
  "total": 0,
  "page": 1,
  "page_size": 20
}
```

## 2. 后台认证接口

### 2.1 登录

```http
POST /api/auth/login
```

请求：

```json
{
  "email": "admin@example.com",
  "password": "Password@123",
  "mfa_code": "123456"
}
```

响应：

```json
{
  "user": {
    "id": "uuid",
    "email": "admin@example.com",
    "name": "Admin"
  },
  "tenant": {
    "id": "uuid",
    "name": "Default"
  },
  "roles": ["admin"],
  "permissions": ["app:read"]
}
```

安全要求：

- 成功后设置 HttpOnly Cookie。
- 不返回 access token 给前端保存。
- 登录失败必须限流。
- MFA 开启时必须校验。

### 2.2 登出

```http
POST /api/auth/logout
```

认证：后台 session。

效果：

- 撤销当前 admin_session。
- 清除 Cookie。

### 2.3 刷新后台 session

```http
POST /api/auth/refresh
```

认证：refresh Cookie。

要求：

- refresh token 轮换。
- 旧 token 标记 used。
- 旧 token 复用撤销整个 session。

响应：

```json
{
  "user": {},
  "tenant": {},
  "roles": [],
  "permissions": []
}
```

### 2.4 当前用户

```http
GET /api/auth/me
```

响应：

```json
{
  "user": {},
  "tenant": {},
  "roles": [],
  "permissions": []
}
```

### 2.5 后台会话列表

```http
GET /api/auth/sessions
```

认证：后台 session。

响应：

```json
{
  "items": [
    {
      "id": "uuid",
      "current": true,
      "status": "active",
      "ip": "127.0.0.1",
      "user_agent": "Mozilla/5.0 ...",
      "created_at": "2026-06-07T12:00:00Z",
      "last_seen_at": "2026-06-07T12:30:00Z",
      "expires_at": "2026-06-08T12:00:00Z",
      "revoked_at": null
    }
  ]
}
```

要求：

- 只返回当前管理员自己的后台会话，最多返回最近 50 条。
- `current=true` 表示当前浏览器正在使用的会话。
- `status` 可为 `active`、`expired`、`revoked`。

### 2.6 撤销后台会话

```http
POST /api/auth/sessions/{id}/revoke
```

认证：后台 session + CSRF。

响应：

```json
{
  "revoked": true,
  "session_id": "uuid",
  "revoked_refresh_tokens": 1
}
```

要求：

- 只能撤销当前管理员自己的其它会话。
- 不能通过该接口撤销当前会话；当前会话应使用登出接口。
- 成功后同时撤销该会话未使用的 refresh token。
- 写审计日志。

### 2.7 修改密码

```http
PUT /api/auth/password
```

请求：

```json
{
  "old_password": "Old@123456",
  "new_password": "New@123456"
}
```

要求：

- 校验旧密码。
- 新密码符合策略。
- 修改成功后撤销其他 session。

### 2.7.1 请求密码重置

```http
POST /api/auth/password/reset/request
```

请求：

```json
{
  "email": "admin@example.com"
}
```

响应：

```json
{
  "ok": true
}
```

要求：

- 无论邮箱是否存在，都返回相同响应，防止枚举账号。
- 写入 `one_time_tokens`，`purpose=admin_password_reset`。
- token 明文只能通过邮件发送，数据库只保存 hash。
- 同一邮箱、同一 IP 必须限流。

### 2.7.2 确认密码重置

```http
POST /api/auth/password/reset/confirm
```

请求：

```json
{
  "token": "reset-token",
  "new_password": "New@123456"
}
```

要求：

- token 必须存在、未过期、未使用、未撤销。
- 新密码必须符合密码策略。
- 成功后写入 `consumed_at`。
- 成功后撤销该管理员所有已有 session 和 refresh token。
- 写审计日志。

### 2.7.3 请求邮箱验证

```http
POST /api/auth/email/verify/request
```

认证：后台 session。

要求：

- 为当前 team_member 创建 `one_time_tokens`，`purpose=email_verify`。
- token 明文只通过邮件发送，数据库只保存 hash。
- 同一账号必须限流。

### 2.7.4 确认邮箱验证

```http
POST /api/auth/email/verify/confirm
```

请求：

```json
{
  "token": "email-verify-token"
}
```

要求：

- token 必须存在、未过期、未使用、未撤销。
- 成功后设置 `team_members.email_verified=true`。
- 成功后写入 `consumed_at`。
- 写审计日志。

### 2.8 MFA 初始化

```http
POST /api/auth/mfa/setup
```

响应：

```json
{
  "secret": "base32",
  "otpauth_url": "otpauth://...",
  "recovery_codes": []
}
```

注意：

- secret 只能在初始化时返回。
- recovery code 只能展示一次。

### 2.9 MFA 确认启用

```http
POST /api/auth/mfa/enable
```

请求：

```json
{
  "code": "123456"
}
```

### 2.10 MFA 关闭

```http
POST /api/auth/mfa/disable
```

请求：

```json
{
  "password": "Password@123",
  "code": "123456"
}
```

### 2.11 重新生成 MFA 恢复码

```http
POST /api/auth/mfa/recovery-codes/regenerate
```

认证：后台 session。

请求：

```json
{
  "password": "Password@123",
  "code": "123456"
}
```

响应：

```json
{
  "recovery_codes": []
}
```

要求：

- 必须已启用 MFA。
- 必须校验当前密码。
- 必须校验当前 TOTP 或未使用的 recovery code。
- 旧 recovery code 全部写入 `revoked_at`。
- 新 recovery code 只展示一次，数据库只保存 hash。
- 写审计日志。

## 3. 租户接口

### 3.1 获取当前租户

```http
GET /api/tenant
```

权限：

```text
tenant:read
```

### 3.2 更新租户

```http
PUT /api/tenant
```

权限：

```text
tenant:update
```

请求：

```json
{
  "name": "公司名称"
}
```

### 3.3 删除租户

```http
DELETE /api/tenant
```

权限：

```text
tenant:delete
```

要求：

- 仅 owner。
- 二次确认。
- 软删除。
- 撤销该租户所有 admin/client session 和 refresh token。
- 记录审计。

## 4. 团队成员接口

### 4.1 成员列表

```http
GET /api/team/members
```

权限：

```text
member:read
```

### 4.2 邀请成员

```http
POST /api/team/invitations
```

权限：

```text
member:invite
```

请求：

```json
{
  "email": "dev@example.com",
  "role_codes": ["developer"]
}
```

### 4.3 接受邀请

```http
POST /api/team/invitations/accept
```

公开接口。

请求：

```json
{
  "token": "invite-token",
  "name": "Developer",
  "password": "Password@123"
}
```

要求：

- `password` 必须符合统一密码策略。

### 4.4 修改成员角色

```http
PUT /api/team/members/{id}/roles
```

权限：

```text
member:update
```

### 4.5 禁用成员

```http
POST /api/team/members/{id}/disable
```

权限：

```text
member:disable
```

要求：

- 不能禁用最后一个 owner。
- 禁用后撤销该成员所有 session 和 refresh token。

### 4.6 角色列表

```http
GET /api/admin/roles
```

权限：

```text
role:read
```

### 4.7 权限点列表

```http
GET /api/admin/permissions
```

权限：

```text
permission:read
```

### 4.8 创建角色

```http
POST /api/admin/roles
```

权限：

```text
role:create
```

请求：

```json
{
  "code": "support",
  "name": "Support",
  "description": "Support staff",
  "permission_codes": ["customer:read"]
}
```

要求：

- role code 租户内唯一。
- 角色变更必须写审计日志。

### 4.9 更新角色

```http
PUT /api/admin/roles/{id}
```

权限：

```text
role:update
```

要求：

- 内置角色不允许修改。
- 角色权限整体替换。
- 必须写审计日志。

### 4.10 删除角色

```http
DELETE /api/admin/roles/{id}
```

权限：

```text
role:delete
```

要求：

- 内置角色不允许删除。
- 已分配给成员的角色不允许删除。
- 软删除并写审计日志。

## 5. 客户接口

### 5.1 客户列表

```http
GET /api/admin/customers
```

权限：

```text
customer:read
```

查询：

```text
keyword
status
page
page_size
```

### 5.2 创建客户

```http
POST /api/admin/customers
```

权限：

```text
customer:create
```

请求：

```json
{
  "email": "user@example.com",
  "name": "User",
  "password": "Password@123"
}
```

要求：

- `password` 可选；提供时必须符合统一密码策略。

### 5.3 更新客户

```http
PUT /api/admin/customers/{id}
```

权限：

```text
customer:update
```

### 5.4 禁用客户

```http
POST /api/admin/customers/{id}/disable
```

权限：

```text
customer:disable
```

要求：

- 禁用后撤销客户相关 client session 和 refresh token。

### 5.5 重置客户密码

```http
POST /api/admin/customers/{id}/reset-password
```

权限：

```text
customer:reset_password
```

响应：

```json
{
  "expires_at": "2026-06-04T10:00:00Z"
}
```

要求：

- 写入 `one_time_tokens`，`purpose=customer_password_reset`。
- token 明文只能通过邮件发送，数据库只保存 hash。
- 不在后台 API 响应或管理端界面展示明文 token。

## 6. 应用接口

### 6.1 应用列表

```http
GET /api/admin/apps
```

权限：

```text
app:read
```

### 6.2 创建应用

```http
POST /api/admin/apps
```

权限：

```text
app:create
```

请求：

```json
{
  "name": "My App",
  "auth_mode": "both",
  "max_devices_default": 1
}
```

响应：

```json
{
  "id": "uuid",
  "app_key": "public-key",
  "app_secret": "show-once"
}
```

注意：

- `app_secret` 只返回一次。
- 后端只保存 hash。
- 自动生成默认签名密钥对，并写入 `signing_keys`。
- 签名密钥通过 `kid` 识别，不直接塞进 `applications`。

### 6.3 应用详情

```http
GET /api/admin/apps/{id}
```

权限：

```text
app:read
```

### 6.4 更新应用

```http
PUT /api/admin/apps/{id}
```

权限：

```text
app:update
```

### 6.5 重新生成密钥

```http
POST /api/admin/apps/{id}/rotate-keys
```

权限：

```text
app:rotate_key
```

注意：

- 会影响客户端验签。
- 必须记录审计。
- 应返回新的 `kid`、public key 和一次性 app_secret。
- 旧 key 进入 `retiring`，不能立刻删除。

### 6.6 应用签名密钥列表

```http
GET /api/admin/apps/{id}/signing-keys
```

权限：

```text
app:read_key
```

响应：

```json
{
  "items": [
    {
      "kid": "release-2026-06-01",
      "key_scope": "release_file",
      "alg": "EdDSA",
      "status": "active",
      "not_before": "...",
      "not_after": null
    }
  ]
}
```

要求：

- 不返回私钥。
- `revoked` key 默认不返回，除非后台显式查询审计历史。

### 6.7 应用公开 JWKS

```http
GET /api/client/apps/{app_key}/jwks
```

认证：无。

用途：

- 客户端验证更新包签名。
- 客户端验证脚本签名。
- 客户端缓存公钥并按 `kid` 选择验签 key。

要求：

- 只返回该应用 `release_file`、`secure_script` 等客户端需要的 public key。
- 不返回私钥。
- `active` 和 `retiring` 状态可以返回。

### 6.8 全局 JWT access token 签名密钥

```http
GET /api/admin/security/jwt-signing-keys
```

权限：

```text
security:read
```

响应：

```json
{
  "items": [
    {
      "kid": "jwt-2026-06-01",
      "key_scope": "jwt_access_token",
      "alg": "EdDSA",
      "status": "active",
      "not_before": "...",
      "not_after": null
    }
  ]
}
```

```http
POST /api/admin/security/jwt-signing-keys/rotate
```

权限：

```text
security:rotate_key
```

响应：

```json
{
  "signing_key": {
    "kid": "jwt-2026-06-05",
    "key_scope": "jwt_access_token",
    "alg": "EdDSA",
    "status": "active",
    "not_before": "...",
    "not_after": null
  },
  "retired_key_count": 1
}
```

要求：

- 不返回私钥。
- 新 key 立即用于签发新的客户端 access token。
- 旧 active key 进入 `retiring`，并写入 `not_after = now + CLIENT_ACCESS_TOKEN_TTL_SECONDS`。
- `/.well-known/jwks.json` 在旧 key 的 `not_after` 前继续暴露公钥，用于存量 access token 验签。
- 必须记录审计。

## 7. 授权接口

### 7.1 授权列表

```http
GET /api/admin/licenses
```

权限：

```text
license:read
```

查询：

```text
app_id
customer_id
status
keyword
```

### 7.2 创建授权

```http
POST /api/admin/licenses
```

权限：

```text
license:create
```

请求：

```json
{
  "app_id": "uuid",
  "customer_id": "uuid",
  "max_devices": 1,
  "expires_at": "2027-01-01T00:00:00Z",
  "features": ["pro"]
}
```

响应：

```json
{
  "license_key": "show-once",
  "license": {}
}
```

### 7.3 吊销授权

```http
POST /api/admin/licenses/{id}/revoke
```

权限：

```text
license:revoke
```

要求：

- 撤销关联设备 session 和 refresh token。
- 记录审计。

### 7.4 暂停授权

```http
POST /api/admin/licenses/{id}/suspend
```

权限：

```text
license:suspend
```

### 7.5 续期授权

```http
POST /api/admin/licenses/{id}/renew
```

权限：

```text
license:renew
```

请求：

```json
{
  "expires_at": "2028-01-01T00:00:00Z"
}
```

### 7.6 重置设备

```http
POST /api/admin/licenses/{id}/reset-devices
```

权限：

```text
license:reset_device
```

请求：

```json
{
  "reason": "customer replaced device"
}
```

要求：

- 请求体不超过 4 KiB，`reason` 必填，去除首尾空白后不超过 500 字符。
- 删除或解绑当前授权下设备。
- 撤销关联设备 session 和 refresh token。
- 撤销相关 client_sessions。
- 记录重置原因和操作人。

## 8. 订阅接口

### 8.1 订阅列表

```http
GET /api/admin/subscriptions
```

权限：

```text
subscription:read
```

### 8.2 创建订阅

```http
POST /api/admin/subscriptions
```

权限：

```text
subscription:create
```

请求：

```json
{
  "app_id": "uuid",
  "customer_id": "uuid",
  "plan": "pro",
  "max_devices": 3,
  "starts_at": "2026-01-01T00:00:00Z",
  "expires_at": "2027-01-01T00:00:00Z",
  "features": ["pro"]
}
```

### 8.3 取消订阅

```http
POST /api/admin/subscriptions/{id}/cancel
```

权限：

```text
subscription:cancel
```

要求：

- 按策略决定立即失效或到期失效。
- 立即失效时撤销相关 session 和 refresh token。

## 9. 设备接口

### 9.1 设备列表

```http
GET /api/admin/devices
```

权限：

```text
device:read
```

查询：

```text
app_id
customer_id
license_id
subscription_id
status
machine_id
```

### 9.2 设备详情

```http
GET /api/admin/devices/{id}
```

权限：

```text
device:read
```

### 9.3 解绑设备

```http
DELETE /api/admin/devices/{id}
```

权限：

```text
device:unbind
```

要求：

- 撤销该设备所有 client session 和 refresh token。

### 9.4 拉黑设备

```http
POST /api/admin/devices/{id}/blacklist
```

权限：

```text
device:blacklist
```

请求：

```json
{
  "reason": "abuse"
}
```

要求：

- `reason` 必填，去除首尾空白后不超过 500 字符。
- 撤销该设备所有 client session 和 refresh token。

### 9.5 解除拉黑

```http
POST /api/admin/devices/{id}/unblacklist
```

权限：

```text
device:unblacklist
```

## 10. 版本分发接口

### 10.1 登记版本文件

```http
POST /api/admin/apps/{id}/release-files
```

权限：

```text
release:upload
```

请求：

```json
{
  "storage_key": "tenants/{tenant_id}/apps/{app_id}/releases/uploads/{upload_id}",
  "file_name": "app.zip",
  "file_size": 123,
  "sha256": "...",
  "metadata": {}
}
```

响应：

```json
{
  "file_id": "uuid",
  "file_name": "app.zip",
  "file_size": 123,
  "sha256": "...",
  "signature_kid": "release-2026-06-01",
  "signature": "...",
  "signature_alg": "Ed25519",
  "file": {
    "id": "uuid",
    "storage_key": "...",
    "file_name": "app.zip",
    "file_size": 123,
    "sha256": "...",
    "signature_kid": "release-2026-06-01",
    "signature": "...",
    "signature_alg": "Ed25519",
    "metadata": {},
    "created_at": "2026-06-01T00:00:00Z"
  }
}
```

要求：

- `storage_key` 可选，未传时服务端生成默认对象路径。
- `file_size` 必须大于 0。
- `sha256` 必须是 64 位十六进制字符串。
- 服务端签名。

### 10.2 上传版本文件

```http
POST /api/admin/apps/{id}/release-files/upload?file_name=app.zip
```

权限：

```text
release:upload
```

请求：`application/octet-stream` 原始文件内容。

响应：同 10.1。

要求：

- 文件内容不能为空。
- 文件大小不超过 100 MB。
- 服务端存储文件、计算 `sha256` 并签名。

### 10.3 版本列表

```http
GET /api/admin/apps/{id}/releases
```

权限：

```text
release:read
```

查询：

- `status`：可选，版本状态。
- `page`：默认 1。
- `page_size`：默认 20，最大 100。

响应：

```json
{
  "items": [
    {
      "id": "uuid",
      "app_id": "uuid",
      "file_id": "uuid",
      "version": "1.0.0",
      "version_code": 100,
      "status": "draft",
      "changelog": "init",
      "force_update": false,
      "published_at": null,
      "deprecated_at": null,
      "created_at": "2026-06-01T00:00:00Z",
      "updated_at": "2026-06-01T00:00:00Z"
    }
  ],
  "meta": {
    "page": 1,
    "page_size": 20
  }
}
```

### 10.4 创建版本

```http
POST /api/admin/apps/{id}/releases
```

权限：

```text
release:create
```

请求：

```json
{
  "file_id": "uuid",
  "version": "1.0.0",
  "version_code": 100,
  "changelog": "init",
  "force_update": false
}
```

### 10.5 发布版本

```http
POST /api/admin/releases/{id}/publish
```

权限：

```text
release:publish
```

### 10.6 废弃版本

```http
POST /api/admin/releases/{id}/deprecate
```

权限：

```text
release:deprecate
```

## 11. 脚本接口

### 11.1 脚本列表

```http
GET /api/admin/apps/{id}/secure-scripts
```

权限：

```text
script:read
```

查询：

- `status`：可选，脚本状态。
- `page`：默认 1。
- `page_size`：默认 20，最大 100。

响应：

```json
{
  "items": [
    {
      "id": "uuid",
      "app_id": "uuid",
      "name": "license-check",
      "version": "1.0.0",
      "version_code": 100,
      "status": "draft",
      "content_sha256": "...",
      "signature_kid": "script-2026-06-01",
      "signature": "...",
      "signature_alg": "Ed25519",
      "required_features": [],
      "expires_at": null,
      "published_at": null
    }
  ],
  "meta": {
    "page": 1,
    "page_size": 20
  }
}
```

### 11.2 创建脚本

```http
POST /api/admin/apps/{id}/secure-scripts
```

权限：

```text
script:create
```

### 11.3 更新脚本内容

```http
POST /api/admin/secure-scripts/{id}/content
```

权限：

```text
script:update
```

要求：

- 服务端加密。
- 计算 hash。
- 签名。
- `content_base64` 解码后大小不超过 1 MiB。

### 11.4 发布脚本

```http
POST /api/admin/secure-scripts/{id}/publish
```

权限：

```text
script:publish
```

### 11.5 废弃脚本

```http
POST /api/admin/secure-scripts/{id}/deprecate
```

权限：

```text
script:deprecate
```

## 12. 审计接口

### 12.1 审计列表

```http
GET /api/admin/audit-logs
```

权限：

```text
audit:read
```

查询：

```text
actor_id
action
resource_type
resource_id
start_at
end_at
```

### 12.2 审计详情

```http
GET /api/admin/audit-logs/{id}
```

权限：

```text
audit:read
```

### 12.3 审计导出

```http
GET /api/admin/audit-logs/export
```

权限：

```text
audit:export
```

查询：

```text
actor_id
action
resource_type
resource_id
start_at
end_at
```

要求：

- 最多导出 1000 条。
- 导出操作必须写入 `audit.export` 审计日志。

## 13. 系统配置接口

### 13.1 系统配置列表

```http
GET /api/admin/system/settings
```

权限：

```text
system:read
```

响应：

```json
{
  "items": [
    {
      "key": "billing.mode",
      "value": {
        "enabled": true
      },
      "updated_at": "2026-06-04T08:30:00Z"
    }
  ]
}
```

### 13.2 更新系统配置

```http
PUT /api/admin/system/settings/{key}
```

权限：

```text
system:update
```

请求：

```json
{
  "value": {
    "enabled": true
  }
}
```

要求：

- 写入 `system_settings`。
- key 只允许字母、数字、`_`、`-`、`.`、`:`。
- `secret`、`password`、`token` 等敏感配置必须继续使用环境变量，不写入系统配置表。
- 更新必须写审计日志。

### 13.3 通知渠道列表

```http
GET /api/admin/notification-channels
```

权限：

```text
notification:read
```

响应：

```json
{
  "items": [
    {
      "id": "00000000-0000-0000-0000-000000000000",
      "name": "ops-webhook",
      "kind": "webhook",
      "enabled": true,
      "config": {
        "target_summary": "https://hooks.example.com"
      },
      "secret_configured": true,
      "last_test_status": "success",
      "last_test_error": null,
      "last_test_at": "2026-06-06T08:00:00Z",
      "created_at": "2026-06-06T07:30:00Z",
      "updated_at": "2026-06-06T08:00:00Z"
    }
  ]
}
```

要求：

- 不返回 webhook URL、SMTP password、PagerDuty routing key 等明文密钥。
- `config` 只放非敏感展示和路由字段。

### 13.4 创建通知渠道

```http
POST /api/admin/notification-channels
```

权限：

```text
notification:update
```

请求：

```json
{
  "name": "ops-webhook",
  "kind": "webhook",
  "enabled": true,
  "config": {},
  "secret": {
    "url": "https://hooks.example.com/alert/token"
  }
}
```

要求：

- `kind` 支持 `webhook`、`email`、`pagerduty`。
- `secret` 使用 `MASTER_KEY` envelope 加密后保存到 `notification_channels.secret_encrypted`。
- 审计日志只记录 `secret_configured`，不能写入明文 secret。

### 13.5 更新通知渠道

```http
PUT /api/admin/notification-channels/{id}
```

权限：

```text
notification:update
```

请求：

```json
{
  "name": "ops-webhook",
  "enabled": true,
  "config": {},
  "secret": {
    "url": "https://hooks.example.com/alert/new-token"
  }
}
```

要求：

- 未传 `secret` 时保留已有密钥。
- `clear_secret=true` 时清空已有密钥。
- 第一版不允许通过更新接口改变 `kind`。

### 13.6 测试通知渠道

```http
POST /api/admin/notification-channels/{id}/test
```

权限：

```text
notification:update
```

要求：

- 默认 `mode=dry_run`：验证必填字段、URL scheme/host、SMTP 字段或 PagerDuty routing key 是否完整，不主动向外部 webhook、SMTP 或值班系统发送测试消息。
- `mode=delivery` 且 `confirm_delivery=true` 时发送一条真实测试消息。Webhook 会收到 `source=admin-test` 的 JSON；Email 会发送测试邮件；PagerDuty 会 trigger 后立即 resolve 同一个 dedup key。
- 更新 `last_test_status`、`last_test_error`、`last_test_at`，并写审计日志。

请求：

```json
{
  "mode": "delivery",
  "confirm_delivery": true
}
```

### 13.7 Alertmanager 内部告警入口

```http
POST /api/internal/alertmanager/webhook
```

认证：

```text
Authorization: Bearer <ALERTMANAGER_WEBHOOK_TOKEN>
```

也兼容 `X-Alertmanager-Token`。该接口不使用后台 session 或 CSRF，只允许 Alertmanager 或内网网关注入。

请求：Alertmanager 标准 webhook JSON，`status` 必须是 `firing` 或 `resolved`。

响应：

```json
{
  "accepted": true,
  "channel_count": 2,
  "delivered": 2,
  "failed": 0,
  "failures": []
}
```

要求：

- `ALERTMANAGER_WEBHOOK_TOKEN` 必须由部署环境配置，长度至少 32 字符。
- 后端读取已启用且已配置密钥的 `notification_channels`，再投递到 webhook、SMTP 或 PagerDuty。
- 业务通知密钥仍只通过后台 `通知渠道` 页面写入数据库加密字段，不写入 Alertmanager YAML。
- 如果存在可投递渠道但全部投递失败，返回 `503 service_unavailable`。

### 13.8 Outbox 事件列表

```http
GET /api/admin/outbox-events
```

权限：

```text
security:view_events
```

查询：

```text
status
event_type
page
page_size
```

要求：

- 按 `created_at desc` 返回。
- 邮件类 payload 必须脱敏，不能返回 `body_envelope` 完整内容。
- 第一版只读，不提供后台删除 outbox 事件。

### 13.9 重试 Outbox 事件

```http
POST /api/admin/outbox-events/{id}/retry
```

权限：

```text
security:retry_event
```

要求：

- 只允许重试 `failed` 状态事件。
- 重试时设置 `status=pending`、`attempts=0`、`next_run_at=now()`。
- 必须写审计日志。

## 14. 客户端认证接口

### 14.1 授权码激活

```http
POST /api/client/auth/activate
```

请求：

```json
{
  "app_key": "app-key",
  "license_key": "license-key",
  "machine_id": "machine-id",
  "device_name": "PC",
  "os": "Windows",
  "app_version": "1.0.0",
  "device_public_key": "public-key"
}
```

响应：

```json
{
  "access_token": "...",
  "refresh_token": "...",
  "token_type": "Bearer",
  "expires_in": 900,
  "refresh_expires_in": 2592000,
  "session_id": "uuid",
  "device_id": "uuid",
  "features": ["pro"]
}
```

`access_token` 是 EdDSA JWT，Header 必须包含 `kid`，可通过 `/.well-known/jwks.json` 获取对应 public key 验签。

必须检查：

- app 存在且 active。
- license 存在且 active。
- license 未过期。
- 设备未超过数量限制。
- machine_id 未被拉黑。
- `device_public_key` 必须是可解析的 Ed25519 public key，支持 PEM 或 base64/base64url 原始 32 字节公钥。

### 14.2 客户账号登录

```http
POST /api/client/auth/login
```

请求：

```json
{
  "app_key": "app-key",
  "email": "user@example.com",
  "password": "Password@123",
  "machine_id": "machine-id",
  "device_public_key": "public-key"
}
```

检查：

- 客户存在且 active。
- 密码正确。
- 客户无需存在有效订阅也可以登录。
- 客户存在有效订阅时绑定订阅并返回功能标记。
- 客户没有有效订阅时仍返回 session，但 `subscription_id=null`、`entitlement_active=false`、`features=[]`。
- 需要订阅的业务接口继续单独校验订阅状态；AI API 必须存在有效订阅才允许调用。
- 有效订阅存在时校验设备数量限制；无有效订阅登录只建立客户会话设备绑定。

响应 `data`：

```json
{
  "access_token": "...",
  "refresh_token": "...",
  "token_type": "Bearer",
  "expires_in": 900,
  "refresh_expires_in": 2592000,
  "session_id": "uuid",
  "device_id": "uuid",
  "device_key_id": "uuid",
  "subscription_id": null,
  "entitlement_id": null,
  "entitlement_kind": null,
  "entitlement_status": "none",
  "entitlement_active": false,
  "features": []
}
```

### 14.2.1 请求客户邮箱验证

```http
POST /api/client/auth/email/verify/request
```

认证：

```text
Authorization Bearer
设备签名
```

要求：

- 为当前 customer 创建 `one_time_tokens`，`purpose=email_verify`。
- token 明文只通过邮件发送，数据库只保存 hash。
- 同一客户、同一 IP 必须限流。

### 14.2.2 确认客户邮箱验证

```http
POST /api/client/auth/email/verify/confirm
```

认证：无。

请求：

```json
{
  "token": "email-verify-token"
}
```

要求：

- token 必须存在、未过期、未使用、未撤销。
- 成功后设置 `customers.email_verified=true`。
- 成功后写入 `consumed_at`。
- 写审计日志或安全事件。

### 14.2.3 确认客户密码重置

```http
POST /api/client/auth/password/reset/confirm
```

认证：无。

请求：

```json
{
  "token": "password-reset-token",
  "new_password": "New@123456"
}
```

要求：

- token 必须存在、未过期、未使用、未撤销，且 `purpose=customer_password_reset`。
- 新密码必须符合密码策略。
- 成功后写入 `consumed_at`。
- 成功后撤销该客户所有 client session 和 refresh token。
- 写审计日志或安全事件。

### 14.3 刷新客户端 session

```http
POST /api/client/auth/refresh
```

请求：

```json
{
  "refresh_token": "..."
}
```

响应同激活。

要求：

- refresh token 轮换。
- 旧 refresh token 复用撤销 session。
- 验证 device、app、license/subscription 状态。

## 15. 客户端保护接口

以下接口需要：

```text
Authorization Bearer
设备签名
nonce 防重放
```

### 15.1 心跳

```http
POST /api/client/auth/heartbeat
```

请求：

```json
{
  "app_version": "1.0.0"
}
```

响应：

```json
{
  "status": "ok",
  "server_time": 1710000000,
  "license_status": "active"
}
```

### 15.2 验证授权

```http
POST /api/client/auth/verify
```

响应：

```json
{
  "valid": true,
  "features": ["pro"],
  "expires_at": "2027-01-01T00:00:00Z"
}
```

### 15.3 注销

```http
POST /api/client/auth/logout
```

效果：

- 撤销当前 client_session。

### 15.4 自助解绑设备

```http
DELETE /api/client/devices/self
```

要求：

- 可以要求客户重新输入密码。
- 撤销当前设备 session 和 refresh token。

### 15.5 设备密钥轮换

```http
POST /api/client/devices/self/rotate-key
```

认证：客户端 session + 设备签名。

请求：

```json
{
  "device_public_key": "-----BEGIN PUBLIC KEY-----..."
}
```

响应：

```json
{
  "device_key_id": "uuid",
  "device_public_key": "-----BEGIN PUBLIC KEY-----...",
  "algorithm": "Ed25519",
  "status": "active",
  "rotated_device_key_ids": ["uuid"]
}
```

说明：

- `device_public_key` 必须是可解析的 Ed25519 public key，支持 PEM 或 base64/base64url 原始 32 字节公钥。
- 轮换请求本身必须使用当前 active 设备 key 签名。
- 服务端按 `X-Device-Key-Id` 精确轮换旧 key，旧 key 标记为 `rotated`，新 key 标记为 `active`。
- 轮换不会撤销当前 session；客户端收到响应后必须保存新私钥和新的 `device_key_id`，后续请求改用新 key 签名。

## 16. 客户端更新接口

### 16.1 获取最新版本

```http
GET /api/client/releases/latest
```

认证：客户端 session。

响应：

```json
{
  "app_id": "uuid",
  "version": "1.0.1",
  "version_code": 101,
  "download_url": "https://...",
  "file_size": 123,
  "sha256": "...",
  "published_at_unix": 1780000000,
  "signature_kid": "release-2026-06-01",
  "signature": "...",
  "signature_alg": "Ed25519",
  "force_update": false
}
```

签名说明：

- `signature_*` 是 release 元数据签名，不是单独的文件登记签名。
- 签名 payload 固定为 `app_id + "\n" + version + "\n" + version_code + "\n" + sha256 + "\n" + file_size + "\n" + published_at_unix`。
- `version_code` 和 `published_at_unix` 进入签名，用于 SDK 防回滚验证。

### 16.2 下载版本文件

```http
GET /api/client/releases/download/{file_name}?token=...
```

要求：

- token 有效。
- token 必须未使用，服务端校验通过后会标记为已使用。
- token 绑定 file_id、device_id、app_id。
- 文件名必须安全。
- 不能支持路径穿越。

## 17. 客户端脚本接口

### 17.1 获取可用脚本版本

```http
GET /api/client/secure-scripts/versions
```

认证：客户端 session。

### 17.2 拉取脚本

```http
POST /api/client/secure-scripts/fetch
```

请求：

```json
{
  "script_id": "uuid"
}
```

响应：

```json
{
  "script_id": "uuid",
  "version": "1.0.0",
  "content_base64": "...",
  "sha256": "...",
  "signature_kid": "script-2026-06-01",
  "signature": "...",
  "signature_alg": "Ed25519",
  "expires_at": "..."
}
```

说明：

- 服务端数据库中加密保存脚本内容。
- 接口返回给客户端的是 `content_base64`。
- 客户端必须验证 `sha256` 和 `signature` 后才能交给业务层。

## 18. 后台 AI 计费管理接口

AI 计费用于配置三方渠道、模型售价、客户余额和客户 API Key。客户端 AI 中转第一版开放 OpenAI 兼容的 `/v1/chat/completions`、`/v1/embeddings`、`/v1/images/generations` 和 `/v1/videos/generations`，调用方只接触 EntitleHub API Key，不暴露三方平台密钥。

### 18.1 AI 渠道列表

```http
GET /api/admin/ai/providers?include_history=false
```

认证：后台 session。

权限：

```text
ai:read
```

响应：

```json
{
  "items": [
    {
      "id": "uuid",
      "name": "OpenAI 主渠道",
      "kind": "openai_compatible",
      "base_url": "https://api.openai.com/v1",
      "enabled": true,
      "config": {},
      "secret_configured": true,
      "created_at": "...",
      "updated_at": "..."
    }
  ]
}
```

### 18.2 创建 AI 渠道

```http
POST /api/admin/ai/providers
```

认证：后台 session + CSRF。

权限：

```text
ai:provider:update
```

请求：

```json
{
  "name": "OpenAI 主渠道",
  "kind": "openai_compatible",
  "base_url": "https://api.openai.com/v1",
  "enabled": true,
  "config": {
    "timeout_ms": 30000
  },
  "secret": {
    "api_key": "sk-..."
  }
}
```

说明：

- `kind` 第一版支持 `openai_compatible`、`custom_http`、`claude`、`gemini`、`deepseek`、`image`、`video`。
- `config` 只能放非敏感配置。
- `secret` 使用 `MASTER_KEY` envelope 加密保存，接口不回显明文。
- 真实三方接口差异通过后续 provider adapter 处理，不在配置里硬编码复杂转换逻辑。

### 18.3 更新 AI 渠道

```http
PUT /api/admin/ai/providers/{id}
```

认证：后台 session + CSRF。

权限：

```text
ai:provider:update
```

请求字段同创建接口，均为可选字段。传入新的 `secret` 会覆盖旧密钥，`clear_secret=true` 会清空已配置密钥。

### 18.4 AI 模型价格列表

```http
GET /api/admin/ai/models?include_history=false&modality=text
```

认证：后台 session。

权限：

```text
ai:read
```

响应：

```json
{
  "items": [
    {
      "id": "uuid",
      "code": "gpt-4o-mini",
      "name": "GPT-4o mini",
      "modality": "text",
      "provider_id": "uuid",
      "provider_name": "OpenAI 主渠道",
      "provider_model": "gpt-4o-mini",
      "enabled": true,
      "currency": "CNY",
      "billing_mode": "token",
      "input_1k_price_minor": 1,
      "output_1k_price_minor": 3,
      "request_price_minor": 0,
      "image_price_minor": 0,
      "second_price_minor": 0,
      "minute_price_minor": 0,
      "daily_spend_limit_minor": null,
      "pricing_config": {},
      "metadata": {}
    }
  ]
}
```

### 18.5 创建 / 更新 AI 模型价格

```http
POST /api/admin/ai/models
PUT /api/admin/ai/models/{id}
```

认证：后台 session + CSRF。

权限：

```text
ai:model:update
```

请求：

```json
{
  "code": "gpt-4o-mini",
  "name": "GPT-4o mini",
  "modality": "text",
  "provider_id": "uuid",
  "provider_model": "gpt-4o-mini",
  "enabled": true,
  "currency": "CNY",
  "billing_mode": "token",
  "input_1k_price_minor": 1,
  "output_1k_price_minor": 3,
  "request_price_minor": 0,
  "image_price_minor": 0,
  "second_price_minor": 0,
  "minute_price_minor": 0,
  "daily_spend_limit_minor": null,
  "pricing_config": {},
  "metadata": {}
}
```

说明：

- 金额字段使用最小货币单位，例如 CNY 分。
- 价格调整只影响新请求，历史调用必须保存价格快照。
- `daily_spend_limit_minor` 为空表示不限制；设置后同一自然日内该模型的新请求预扣金额不能超过该限额。
- `modality` 支持 `text`、`image`、`video`、`audio`、`embedding`、`multimodal`。
- `billing_mode` 控制生效价格字段：`token` 使用 `input_1k_price_minor` / `output_1k_price_minor`，`per_image` 使用 `image_price_minor`，`video_per_second` 使用 `second_price_minor`，`video_per_request` 使用 `request_price_minor`，`audio_per_second` 使用 `second_price_minor`，`audio_per_minute` 使用 `minute_price_minor`，`audio_per_request` 使用 `request_price_minor`。
- `pricing_config` 预留给三方特殊计费参数，必须是普通 JSON 对象，不要放密钥。

### 18.6 AI 钱包列表

```http
GET /api/admin/ai/wallets?include_history=false
```

认证：后台 session。

权限：

```text
ai:read
```

响应：

```json
{
  "items": [
    {
      "customer_id": "uuid",
      "customer_email": "user@example.com",
      "customer_name": "User",
      "wallet_id": "uuid",
      "currency": "CNY",
      "balance_minor": 10000,
      "held_minor": 0,
      "available_minor": 10000,
      "daily_spend_limit_minor": null,
      "updated_at": "..."
    }
  ]
}
```

### 18.7 手动调整 AI 钱包余额

```http
POST /api/admin/ai/customers/{id}/wallet/adjust
```

认证：后台 session + CSRF。

权限：

```text
ai:wallet:update
```

请求：

```json
{
  "amount_minor": 10000,
  "reason": "后台充值",
  "metadata": {}
}
```

说明：

- 正数表示充值，负数表示扣减。
- 后端会自动创建客户钱包。
- 余额流水会写入 `ai_wallet_ledger_entries`。
- 扣减后余额不能小于冻结金额。
- 后续网关发起请求时使用 `hold` 预扣，三方成功后 `capture` 结算，失败后 `release/refund` 退款。

### 18.7.1 更新 AI 钱包每日限额

```http
PUT /api/admin/ai/customers/{id}/wallet/quota
```

认证：后台 session + CSRF。

权限：

```text
ai:wallet:update
```

请求：

```json
{
  "daily_spend_limit_minor": 50000
}
```

说明：

- 金额使用最小货币单位；传 `null` 表示清空限制。
- 限额按客户钱包维度生效，同一自然日内“已成功扣费 + 正在预扣”的金额达到上限后，新请求会被拒绝。

### 18.8 AI 钱包流水

```http
GET /api/admin/ai/customers/{id}/wallet/ledger?page=1&page_size=20
```

认证：后台 session。

权限：

```text
ai:read
```

响应：

```json
{
  "items": [
    {
      "id": "uuid",
      "customer_id": "uuid",
      "entry_type": "credit",
      "amount_minor": 10000,
      "balance_after_minor": 10000,
      "held_after_minor": 0,
      "reason": "后台充值",
      "metadata": {},
      "created_at": "..."
    }
  ],
  "meta": {
    "page": 1,
    "page_size": 20
  }
}
```

### 18.9 AI API Key 列表

```http
GET /api/admin/ai/api-keys?include_history=false&customer_id=uuid
```

认证：后台 session。

权限：

```text
ai:read
```

响应：

```json
{
  "items": [
    {
      "id": "uuid",
      "customer_id": "uuid",
      "customer_email": "user@example.com",
      "customer_name": "User",
      "name": "生产环境 SDK Key",
      "key_prefix": "ehai_xxxxxxxxxxxxx",
      "status": "active",
      "expires_at": null,
      "daily_spend_limit_minor": null,
      "last_used_at": "...",
      "created_at": "...",
      "revoked_at": null
    }
  ]
}
```

说明：只返回 Key 前缀，不返回明文。

### 18.10 创建 / 吊销 AI API Key

```http
POST /api/admin/ai/customers/{id}/api-keys
POST /api/admin/ai/api-keys/{id}/revoke
```

认证：后台 session + CSRF。

权限：

```text
ai:api_key:update
```

创建请求：

```json
{
  "name": "生产环境 SDK Key",
  "expires_at": null,
  "daily_spend_limit_minor": null
}
```

创建响应：

```json
{
  "api_key": {
    "id": "uuid",
    "customer_id": "uuid",
    "customer_email": "user@example.com",
    "name": "生产环境 SDK Key",
    "key_prefix": "ehai_xxxxxxxxxxxxx",
    "status": "active",
    "created_at": "..."
  },
  "plain_key": "ehai_..."
}
```

说明：

- `plain_key` 只返回一次，数据库只保存 hash。
- 只能给 active 客户创建 Key。
- 吊销后客户端立即无法继续调用 AI 网关。
- `daily_spend_limit_minor` 为空表示不限制；设置后同一自然日内该 Key 的新请求预扣金额不能超过该限额。

### 18.10.1 更新 AI API Key

```http
PUT /api/admin/ai/api-keys/{id}
```

认证：后台 session + CSRF。

权限：

```text
ai:api_key:update
```

请求：

```json
{
  "name": "生产环境 SDK Key",
  "expires_at": null,
  "daily_spend_limit_minor": 20000
}
```

说明：

- 字段均为可选；`expires_at` 或 `daily_spend_limit_minor` 传 `null` 表示清空。
- 更新不会返回明文 Key。

### 18.10.2 服务端 Server Key 管理

Server Key 用于 Web 后端、业务服务端调用 EntitleHub。它绑定到一个应用，不绑定单个客户；调用时由业务后端通过请求头传入本次实际消费的客户 ID。浏览器和移动端页面不能保存或直连使用 Server Key。

```http
GET /api/admin/server-api-keys?include_history=false&app_id=uuid
POST /api/admin/server-api-keys
PUT /api/admin/server-api-keys/{id}
POST /api/admin/server-api-keys/{id}/revoke
```

认证：后台 session；创建、更新、吊销需要 CSRF。

权限：

```text
server_api_key:read
server_api_key:update
```

创建请求：

```json
{
  "app_id": "uuid",
  "name": "影织 Web 后端",
  "scopes": ["ai:invoke"],
  "expires_at": null
}
```

创建响应：

```json
{
  "server_api_key": {
    "id": "uuid",
    "app_id": "uuid",
    "app_name": "影织",
    "app_key": "app_xxx",
    "name": "影织 Web 后端",
    "key_prefix": "ehsk_xxxxxxxxxxxxx",
    "status": "active",
    "scopes": ["ai:invoke"],
    "expires_at": null,
    "last_used_at": null,
    "created_at": "...",
    "revoked_at": null
  },
  "plain_key": "ehsk_..."
}
```

说明：

- `plain_key` 只返回一次，数据库只保存 hash。
- Server Key 只能为 active 应用创建；应用禁用、Key 过期或吊销后立即不可用。
- 当前作用域为 `ai:invoke`，后续视频、文件等能力可以继续扩展 scope。
- 生产建议一个产品后端使用一个 Server Key；多产品分别创建，避免一个 Key 横跨所有产品。

### 18.10.3 Web 后端 AI 转发入口

Web 产品推荐由“你的业务后端”调用 EntitleHub，浏览器只登录你的业务系统，不保存 EntitleHub Server Key。

```http
POST /api/server/ai/v1/chat/completions
POST /api/server/ai/v1/embeddings
POST /api/server/ai/v1/images/generations
POST /api/server/ai/v1/videos/generations
POST /api/server/ai/v1/images/jobs
POST /api/server/ai/v1/videos/jobs
GET /api/server/ai/v1/jobs/{job_id}
GET /api/server/ai/v1/models
Authorization: Bearer ehsk_...
X-EntitleHub-Customer-Id: uuid
Content-Type: application/json
```

调用规则：

- `Authorization` 使用后台创建的 Server Key。
- `X-EntitleHub-Customer-Id` 是本次消费的客户 ID。
- Server Key 绑定的应用下，客户必须有 active/trialing 且未过期的订阅，否则返回订阅不可用。
- 客户必须是 active，且 AI 钱包未冻结、余额足够。
- 计费、失败退款、图片/视频缓存、模型校验和 `/v1/...` 客户级 AI API Key 网关一致。
- 幂等键仍使用 `Idempotency-Key`；服务端入口写入独立 endpoint，例如 `/api/server/ai/v1/chat/completions`，不会和客户端或客户级 `/v1` 入口互相重放。
- 图片/视频异步平台优先使用 `/images/jobs` 和 `/videos/jobs`：EntitleHub 创建自己的任务、冻结余额、提交三方任务、后台轮询三方结果、成功后缓存素材并确认扣费，三方失败则释放预扣金额。

示例：

```bash
curl https://your-domain.example/api/server/ai/v1/chat/completions \
  -H "Authorization: Bearer ehsk_..." \
  -H "X-EntitleHub-Customer-Id: 00000000-0000-0000-0000-000000000001" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [{"role": "user", "content": "hello"}],
    "max_tokens": 128
  }'
```

### 18.10.4 Web 产品平台 API

给影织这类 Web SaaS 后端使用。浏览器仍然只调用影织后端，影织后端用 Server Key 调 EntitleHub。

```http
POST /api/server/web/v1/customers/register
POST /api/server/web/v1/customers/login
GET /api/server/web/v1/customers/{customer_id}
GET /api/server/web/v1/customers/{customer_id}/balance
GET /api/server/web/v1/customers/{customer_id}/usage
GET /api/server/web/v1/customers/{customer_id}/plan
GET /api/server/web/v1/ai/models?type=image|video
POST /api/server/web/v1/ai/jobs
GET /api/server/web/v1/ai/jobs?customer_id={customer_id}
GET /api/server/web/v1/ai/jobs/{job_id}?customer_id={customer_id}
POST /api/server/web/v1/ai/jobs/{job_id}/cancel
POST /api/server/web/v1/ai/jobs/{job_id}/retry
POST /api/server/web/v1/assets/upload
POST /api/server/web/v1/works/{work_id}/download
Authorization: Bearer ehsk_...
Content-Type: application/json
```

说明：

- 注册/登录只校验 EntitleHub 客户账号；Web 浏览器 session 由影织后端自己维护。
- `GET /ai/models` 不需要客户 ID，用于渲染模型商品、比例、分辨率、时长、价格等前端选项。
- `POST /ai/jobs` 必须在请求体传 `customer_id`，EntitleHub 以该客户做订阅校验、钱包预扣和任务归属。
- `cancel` 是 EntitleHub 本地取消：停止继续轮询并释放未扣费预扣；如果三方平台没有取消接口，不能保证三方侧停止生成。
- `retry` 是重新查询第三方任务，不是新建任务；重新生成应重新调用 `POST /api/server/web/v1/ai/jobs` 并使用新的幂等键。
- 用户上传素材和 AI 生成结果统一进入 Web 资产库，影织可以按客户、文件夹、类型、用途查询。
- 模型 `capabilities` 会返回 `inputModes`、`maxReferenceImages`、`supportsReferenceVideo`、`supportsFirstFrame`、`supportsLastFrame`、`acceptedMimeTypes`、`maxAssetSizeMb`，Web 产品应按这些字段渲染参考素材、首帧、尾帧等输入能力。

统一创建视频任务示例：

```json
{
  "customer_id": "00000000-0000-0000-0000-000000000001",
  "type": "video",
  "model": "yingzhi-video-fast",
  "prompt": "一个 8 秒产品展示视频",
  "ratio": "16:9",
  "resolution": "1080p",
  "duration": 8
}
```

带参考素材的视频任务示例：

```json
{
  "customer_id": "00000000-0000-0000-0000-000000000001",
  "type": "video",
  "model": "yingzhi-video-fast",
  "prompt": "根据首尾帧生成视频",
  "inputMode": "frames",
  "referenceAssetIds": ["00000000-0000-0000-0000-000000000002"],
  "firstFrameAssetId": "00000000-0000-0000-0000-000000000003",
  "lastFrameAssetId": "00000000-0000-0000-0000-000000000004",
  "aspectRatio": "16:9",
  "resolution": "1080p",
  "durationSec": 8
}
```

说明：

- 参考素材只支持视频任务。
- `referenceAssetIds`、`firstFrameAssetId`、`lastFrameAssetId` 必须属于当前 Server Key 应用下的当前客户。
- EntitleHub 会校验素材状态、类型、MIME、大小和模型能力。
- 任务、作品和广场返回会包含 `sourceMode`、`referenceCount`、`hasFirstFrame`、`hasLastFrame`、`publishedAt`、`favoritedAt`、`downloadedAt`。

完整接入流程见 `Web 后端接入指南.md`。

### 18.10.5 Web 产品资产库接口

资产库给 Web 产品管理用户素材和生成结果。浏览器不保存 Server Key；需要上传时，由 Web 后端创建短期上传会话，再把一次性上传令牌给浏览器。

```http
GET    /api/server/web/v1/asset-folders?customer_id=uuid
POST   /api/server/web/v1/asset-folders
PATCH  /api/server/web/v1/asset-folders/{id}
DELETE /api/server/web/v1/asset-folders/{id}

POST   /api/server/web/v1/assets/upload-url
POST   /api/server/web/v1/assets/upload
PUT    /api/server/web/v1/assets/uploads/{id}
GET    /api/server/web/v1/assets?customer_id=uuid
GET    /api/server/web/v1/assets/{id}
PATCH  /api/server/web/v1/assets/{id}
DELETE /api/server/web/v1/assets/{id}
GET    /api/server/web/v1/assets/{id}/download
```

认证：

```text
管理/查询接口：Authorization: Bearer ehsk_...
上传字节接口：X-EntitleHub-Upload-Token: ehup_...
```

创建上传会话：

```json
{
  "customer_id": "uuid",
  "folder_id": null,
  "file_name": "reference.png",
  "asset_type": "image",
  "asset_role": "reference",
  "mime_type": "image/png",
  "file_size": 123456,
  "metadata": {}
}
```

上传字节：

```http
PUT /api/server/web/v1/assets/uploads/{id}
X-EntitleHub-Upload-Token: ehup_...
Content-Type: image/png

<binary>
```

服务端直传：

```http
POST /api/server/web/v1/assets/upload?customer_id=uuid&file_name=reference.png&asset_type=image&asset_role=reference&mime_type=image/png
Authorization: Bearer ehsk_...
Content-Type: image/png

<binary>
```

上传成功响应会包含完整 `asset`，并额外返回 Web 友好别名：

```json
{
  "asset": {
    "id": "uuid",
    "asset_type": "image",
    "asset_role": "reference",
    "public_url": "https://example.com/api/server/web/v1/assets/uuid/download",
    "mime_type": "image/png"
  },
  "assetId": "uuid",
  "url": "https://example.com/api/server/web/v1/assets/uuid/download",
  "type": "image",
  "mimeType": "image/png"
}
```

资产类型：

- `asset_type`：`image`、`video`、`audio`、`file`。
- `asset_role`：`upload`、`generated`、`reference`、`first_frame`、`last_frame`、`brand`、`other`。
- `source`：`user_upload`、`generated`、`imported`。

说明：

- 上传令牌默认 15 分钟有效，只能使用一次。
- 上传文件最大 512 MB。
- 文件夹删除要求文件夹为空。
- 资产删除是软删除，不会立刻物理删除对象存储文件。
- Server Key 调用的同步/异步图片视频生成成功后，会自动把生成素材写入资产库，`asset_role=generated`、`source=generated`，并自动创建私有作品。
- 参考素材提交给第三方时会使用资产 `public_url`。生产环境应使用第三方可访问的对象存储公开 URL 或短期签名 URL；如果 `public_url` 是需要 Server Key 的 EntitleHub 下载接口，第三方平台通常无法直接拉取。

### 18.10.6 Web 产品作品接口

作品接口给影织实现“我的作品、我的收藏、灵感广场、详情页”。作品不是资产标签，而是独立业务对象：

- `Asset`：文件本身。
- `Work`：用户作品，指向主资产和可选封面资产。
- `Favorite`：某个客户收藏某个作品。
- `Publication`：作品发布到广场的状态。

接口：

```http
GET    /api/server/web/v1/works?customer_id=uuid
GET    /api/server/web/v1/works?customer_id=uuid&favorite=true
GET    /api/server/web/v1/works/{id}?customer_id=uuid
PATCH  /api/server/web/v1/works/{id}
DELETE /api/server/web/v1/works/{id}
POST   /api/server/web/v1/works/{id}/favorite
DELETE /api/server/web/v1/works/{id}/favorite
POST   /api/server/web/v1/works/{id}/download
POST   /api/server/web/v1/works/{id}/publish
POST   /api/server/web/v1/works/{id}/unpublish
GET    /api/server/web/v1/gallery?type=video&customer_id=uuid
Authorization: Bearer ehsk_...
Content-Type: application/json
```

说明：

- 图片/视频生成成功后，EntitleHub 自动创建 `Work`，默认 `visibility=private`。
- `GET /works` 默认返回当前客户拥有的作品；`favorite=true` 返回当前客户收藏的作品。
- 发布到广场后，`GET /gallery` 只返回当前 Server Key 绑定应用下 `published` 的作品。
- `customer_id` 传给 `gallery` 时，返回字段中的 `favorited` 会按该客户计算。
- 删除作品是软删除，并会从广场下架；不会删除底层资产文件。
- 发布、取消发布、更新、删除只能由作品 owner 操作。
- `download` 会记录当前客户下载过该作品，并返回作品主资产下载地址；下载状态是客户维度，返回字段为 `downloadedAt`。

更新作品请求：

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

收藏/取消收藏/取消发布请求：

```json
{
  "customer_id": "uuid"
}
```

发布请求：

```json
{
  "customer_id": "uuid",
  "tags": ["产品展示", "科技感"]
}
```

下载请求：

```json
{
  "customer_id": "uuid"
}
```

返回核心字段：

```json
{
  "work": {
    "id": "uuid",
    "owner_customer_id": "uuid",
    "source_job_id": "uuid",
    "title": "AI 生成视频",
    "work_type": "video",
    "visibility": "private",
    "primary_asset_id": "uuid",
    "primary_asset_url": "https://example.com/api/ai/assets/...",
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
      "sourceMode": "frames",
      "referenceCount": 2,
      "hasFirstFrame": true,
      "hasLastFrame": true
    }
  }
}
```

### 18.10.7 Web 后端异步图片/视频任务

异步任务用于速创等“提交后返回任务 ID、再查询结果”的图片/视频平台。任务状态以第三方查询接口返回为准，EntitleHub 只负责内部商业状态：余额冻结、扣费/释放、素材缓存、后台审计。

创建图片任务：

```http
POST /api/server/ai/v1/images/jobs
Authorization: Bearer ehsk_...
X-EntitleHub-Customer-Id: uuid
Idempotency-Key: optional-unique-key
Content-Type: application/json
```

请求示例：

```json
{
  "model": "image-gpt",
  "prompt": "一张产品海报",
  "n": 1,
  "size": "1024x1024"
}
```

创建视频任务：

```http
POST /api/server/ai/v1/videos/jobs
Authorization: Bearer ehsk_...
X-EntitleHub-Customer-Id: uuid
Idempotency-Key: optional-unique-key
Content-Type: application/json
```

请求示例：

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

查询任务：

```http
GET /api/server/ai/v1/jobs/{job_id}
Authorization: Bearer ehsk_...
X-EntitleHub-Customer-Id: uuid
```

任务状态：

- `submitted` / `running`：三方还在生成。
- `caching`：三方已成功，EntitleHub 正在缓存图片/视频。
- `succeeded`：生成成功、素材已缓存、余额已确认扣费。
- `provider_failed` / `failed`：三方失败或不可恢复错误，预扣金额已释放。
- `timeout_review`：长时间没有最终结果，后台待人工确认；系统不会自行猜测失败。

速创接入建议：

- AI 渠道类型选择 `速创平台`，`base_url` 填 `https://api.wuyinkeji.com`。
- 渠道密钥填三方 API Key；默认使用 `Authorization: <api_key>`。如三方要求 `Bearer` 或其他 header，可在渠道配置中设置 `api_key_header`、`auth_scheme` 或 `headers`。
- 任务查询默认使用 `GET /api/async/detail?id=<task_id>`；如三方要求 POST，可在渠道配置中设置 `detail_method: "POST"`、`detail_path`、`detail_id_field`。
- 模型 `provider_model` 可用 `google_omni`、`grok_imagine`、`image_gpt`、`image_nanoBanana2`；也可以在模型 `pricing_config.submit_path` 显式配置提交路径。

### 18.10.8 后台生成任务处理

后台生成任务用于运维处理异步图片/视频任务异常，包括查看三方原始返回、重新查询三方、重新缓存素材、失败释放预扣和人工退款。

```http
GET /api/admin/ai/generation-jobs?page=1&page_size=50
GET /api/admin/ai/generation-jobs/{id}
POST /api/admin/ai/generation-jobs/{id}/retry-poll
POST /api/admin/ai/generation-jobs/{id}/retry-cache
POST /api/admin/ai/generation-jobs/{id}/fail-release
POST /api/admin/ai/generation-jobs/{id}/refund
```

认证：后台 session；操作类接口需要 CSRF。

权限：

```text
ai:job:read
ai:job:update
```

操作请求：

```json
{
  "reason": "后台人工处理原因"
}
```

说明：

- 详情接口会返回 `request_payload`、`provider_submit_response`、`provider_result_response`，用于排查三方任务状态。
- `retry-poll` 只把任务重新加入查询队列，最终状态仍以三方查询接口返回为准。
- `retry-cache` 使用已保存的三方结果重新下载并缓存素材；已成功扣费的任务不能重复缓存，避免重复扣费。
- `fail-release` 只适用于未结算任务，会把冻结余额释放回客户可用余额，并把任务标记为失败。
- `refund` 只适用于已成功扣费任务，会把已扣金额退回客户 AI 余额，写入退款流水，并保留后台审计日志。
- Viewer 角色只能查看任务；Owner、Admin、Developer 可以执行处理动作。

### 18.11 OpenAI 兼容 Chat Completions 网关

```http
POST /v1/chat/completions
Authorization: Bearer ehai_...
Content-Type: application/json
```

认证：客户 AI API Key。

请求：保持 OpenAI Chat Completions 格式，`model` 使用后台 `AI 模型价格` 中配置的对外模型代码。

```json
{
  "model": "gpt-4o-mini",
  "messages": [
    {
      "role": "user",
      "content": "hello"
    }
  ],
  "max_tokens": 128
}
```

响应：保持三方 OpenAI 兼容接口返回格式，并附加响应头：

```text
x-entitlehub-usage-id: uuid
```

计费规则：

- 请求开始时按模型价格预估并冻结客户 AI 钱包余额。
- `billing_mode=token` 时，根据后台输入 / 输出 token 单价预扣。
- 三方返回 2xx 成功后，根据三方 `usage.prompt_tokens` / `usage.completion_tokens` 结算；没有 usage 时按预扣金额结算。
- 三方返回失败或请求异常时释放预扣金额。
- 三方平台状态和响应以三方回传为准，调用记录写入 `ai_usage_records`。
- 第一版网关只支持 `openai_compatible` 且 `modality=text|multimodal` 的模型。
- 第一版暂不支持 `stream=true`，传入流式请求会返回参数错误。
- AI API Key 默认每 60 秒最多 120 次网关请求，可通过 `AI_GATEWAY_RATE_LIMIT_MAX` 和 `AI_GATEWAY_RATE_LIMIT_WINDOW_SECONDS` 调整。
- 可传 `Idempotency-Key` 请求头，长度 1-200。相同客户、Key、endpoint 和幂等键的已完成请求会直接返回上次三方响应；仍在处理中的请求会返回冲突错误，避免重复扣费。

### 18.12 OpenAI 兼容 Embeddings 网关

```http
POST /v1/embeddings
Authorization: Bearer ehai_...
Content-Type: application/json
```

认证：客户 AI API Key。

请求：保持 OpenAI Embeddings 格式，`model` 使用后台 `AI 模型价格` 中配置的对外模型代码。

```json
{
  "model": "text-embedding-3-small",
  "input": "hello"
}
```

响应：保持三方 OpenAI 兼容接口返回格式，并附加响应头：

```text
x-entitlehub-usage-id: uuid
```

计费规则：

- `billing_mode=token` 时，请求开始按预估输入 token 和 `input_1k_price_minor` 冻结客户 AI 钱包余额。
- 三方返回 2xx 成功后，根据三方 `usage.prompt_tokens` 结算；没有 `prompt_tokens` 时使用 `usage.total_tokens`；没有 usage 时按预扣金额结算。
- 三方返回失败或请求异常时释放预扣金额。
- 第一版只支持 `openai_compatible` 且 `modality=embedding|multimodal` 的模型。
- AI API Key 默认每 60 秒最多 120 次网关请求，可通过 `AI_GATEWAY_RATE_LIMIT_MAX` 和 `AI_GATEWAY_RATE_LIMIT_WINDOW_SECONDS` 调整。
- 支持 `Idempotency-Key`，规则同 Chat Completions。

### 18.13 OpenAI 兼容 Images Generations 网关

```http
POST /v1/images/generations
Authorization: Bearer ehai_...
Content-Type: application/json
```

认证：客户 AI API Key。

请求：保持 OpenAI Images Generations 格式，`model` 使用后台 `AI 模型价格` 中配置的对外模型代码。

```json
{
  "model": "image-test",
  "prompt": "一张产品海报",
  "n": 1,
  "size": "1024x1024"
}
```

响应：保持三方兼容格式，并附加响应头：

```text
x-entitlehub-usage-id: uuid
```

返回体里的图片会被缓存到 EntitleHub 对象存储。三方返回 `data[].url` 时会替换成平台自有地址；三方返回 `data[].b64_json` 时会写入对象存储，并改为返回平台自有 `url`。

示例：

```json
{
  "created": 1710000000,
  "data": [
    {
      "url": "https://your-domain.example/api/ai/assets/uuid"
    }
  ]
}
```

计费规则：

- `billing_mode=per_image` 时，请求开始按 `image_price_minor * n` 预扣，`n` 默认 1，最大 10。
- 三方返回 2xx 且图片缓存成功后，优先按三方返回 `data` 数量和 `image_price_minor` 结算；返回体没有可识别图片数量时按预扣金额结算。
- 三方失败、请求异常或图片缓存失败时释放预扣金额。
- 第一版只支持 `openai_compatible` 且 `modality=image|multimodal` 的模型。
- AI API Key 默认每 60 秒最多 120 次网关请求，可通过 `AI_GATEWAY_RATE_LIMIT_MAX` 和 `AI_GATEWAY_RATE_LIMIT_WINDOW_SECONDS` 调整。
- 支持 `Idempotency-Key`，规则同 Chat Completions；幂等重放返回已缓存后的平台素材地址。

### 18.13.1 OpenAI 兼容 Videos Generations 网关

```http
POST /v1/videos/generations
Authorization: Bearer ehai_...
Content-Type: application/json
```

认证：客户 AI API Key。

请求：保持 OpenAI 兼容或三方视频生成 JSON 格式，`model` 使用后台 `AI 模型价格` 中配置的对外模型代码。

```json
{
  "model": "video-test",
  "prompt": "一个 8 秒产品展示视频",
  "duration": 8,
  "size": "1280x720"
}
```

响应：保持三方兼容格式，并附加响应头：

```text
x-entitlehub-usage-id: uuid
```

返回体里的视频 URL 会被缓存到 EntitleHub 对象存储。第一版识别并替换响应 JSON 中的 `url`、`video_url`、`output_url`、`download_url` 字段，替换后的值为平台自有 `/api/ai/assets/{id}` 地址。

示例：

```json
{
  "created": 1710000000,
  "data": [
    {
      "video_url": "https://your-domain.example/api/ai/assets/uuid"
    }
  ]
}
```

计费规则：

- `billing_mode=video_per_request` 时，请求开始按 `request_price_minor` 预扣，成功后按次结算。
- `billing_mode=video_per_second` 时，请求开始按 `request_price_minor + second_price_minor * duration` 预扣；`duration` 可用 `duration`、`duration_seconds` 或 `seconds`，默认 8 秒，最大 3600 秒。
- 三方返回 2xx 且视频缓存成功后，优先按三方响应中的 `duration` / `duration_seconds` / `seconds` 结算；无法识别时按模型 `pricing_config.default_duration_seconds`，仍无法识别时按预扣金额结算。
- 三方失败、请求异常或视频缓存失败时释放预扣金额。
- 同步视频网关只支持同步返回视频 URL 的 `openai_compatible` 三方接口，且模型 `modality=video|multimodal`。速创等异步任务型视频平台请使用 `/api/server/ai/v1/videos/jobs`。
- AI API Key 默认每 60 秒最多 120 次网关请求，可通过 `AI_GATEWAY_RATE_LIMIT_MAX` 和 `AI_GATEWAY_RATE_LIMIT_WINDOW_SECONDS` 调整。
- 支持 `Idempotency-Key`，规则同 Chat Completions；幂等重放返回已缓存后的平台素材地址。

### 18.14 AI 生成素材访问

```http
GET /api/ai/assets/{id}
```

该地址由图片/视频生成网关返回，用于下发已缓存到 EntitleHub 对象存储的生成素材。接口返回原始文件字节，并设置 `Content-Type`、`Content-Length` 和长期缓存头。

说明：

- 素材 ID 为 UUID，客户端不需要三方平台地址或密钥。
- 当前用于图片、同步视频和异步任务结果缓存；音频、文件类素材后续可复用同一张 `ai_assets` 表扩展。

### 18.14.1 后台缓存素材管理

```http
GET /api/admin/ai/assets?status=ready&asset_type=image&customer_id=uuid&page=1&page_size=50
DELETE /api/admin/ai/assets/{id}
```

认证：后台 session；删除需要 CSRF。

权限：

```text
ai:read
ai:asset:delete
```

列表响应：

```json
{
  "items": [
    {
      "id": "uuid",
      "usage_id": "uuid",
      "customer_email": "user@example.com",
      "provider_name": "OpenAI 主渠道",
      "model_code": "image-test",
      "asset_type": "image",
      "status": "ready",
      "public_url": "https://your-domain.example/api/ai/assets/uuid",
      "mime_type": "image/png",
      "file_size": 1024,
      "created_at": "...",
      "updated_at": "...",
      "deleted_at": null
    }
  ],
  "meta": {
    "page": 1,
    "page_size": 50
  }
}
```

说明：

- 不传 `status` 时默认不返回已删除素材。
- 删除为软删除，状态改为 `deleted`，客户端素材访问接口不再返回该素材。

### 18.15 OpenAI 兼容模型列表

```http
GET /v1/models
Authorization: Bearer ehai_...
```

认证：客户 AI API Key。

响应：

```json
{
  "object": "list",
  "data": [
    {
      "id": "gpt-4o-mini",
      "object": "model",
      "created": 1710000000,
      "owned_by": "entitlehub"
    }
  ]
}
```

说明：只返回当前租户启用、且已绑定启用渠道的模型。

AI API Key 默认每 60 秒最多 120 次网关请求，模型列表接口也计入同一限流窗口。

### 18.16 AI 调用记录列表

```http
GET /api/admin/ai/usage-records?status=succeeded&customer_id=uuid&page=1&page_size=50
```

认证：后台 session。

权限：

```text
ai:read
```

响应：

```json
{
  "items": [
    {
      "id": "uuid",
      "customer_email": "user@example.com",
      "provider_name": "OpenAI 主渠道",
      "model_code": "gpt-4o-mini",
      "endpoint": "/v1/chat/completions",
      "status": "succeeded",
      "provider_status": "200",
      "prompt_tokens": 100,
      "completion_tokens": 200,
      "total_tokens": 300,
      "charged_minor": 10,
      "refunded_minor": 0,
      "currency": "CNY",
      "created_at": "...",
      "completed_at": "..."
    }
  ],
  "meta": {
    "page": 1,
    "page_size": 50
  }
}
```

### 18.17 AI 网关客户端接入示例

客户侧只需要 EntitleHub AI API Key，不需要三方平台密钥。后台把三方渠道、模型和价格配置好后，客户把 OpenAI 兼容 SDK 的 `base_url` / `baseURL` 指向 EntitleHub 即可。

基础配置：

```text
base_url=https://your-domain.example/v1
api_key=ehai_...
```

HTTP Chat 示例：

```bash
curl https://your-domain.example/v1/chat/completions \
  -H "Authorization: Bearer ehai_..." \
  -H "Content-Type: application/json" \
  -d '{
    "model": "gpt-4o-mini",
    "messages": [
      {"role": "user", "content": "hello"}
    ],
    "max_tokens": 128
  }'
```

HTTP Embeddings 示例：

```bash
curl https://your-domain.example/v1/embeddings \
  -H "Authorization: Bearer ehai_..." \
  -H "Content-Type: application/json" \
  -d '{
    "model": "text-embedding-3-small",
    "input": "hello"
  }'
```

HTTP Images 示例：

```bash
curl https://your-domain.example/v1/images/generations \
  -H "Authorization: Bearer ehai_..." \
  -H "Content-Type: application/json" \
  -d '{
    "model": "image-test",
    "prompt": "一张产品海报",
    "n": 1,
    "size": "1024x1024"
  }'
```

HTTP Videos 示例：

```bash
curl https://your-domain.example/v1/videos/generations \
  -H "Authorization: Bearer ehai_..." \
  -H "Content-Type: application/json" \
  -d '{
    "model": "video-test",
    "prompt": "一个 8 秒产品展示视频",
    "duration": 8,
    "size": "1280x720"
  }'
```

Python OpenAI SDK 示例：

```python
from openai import OpenAI

client = OpenAI(
    api_key="ehai_...",
    base_url="https://your-domain.example/v1",
)

chat = client.chat.completions.create(
    model="gpt-4o-mini",
    messages=[{"role": "user", "content": "hello"}],
)

embedding = client.embeddings.create(
    model="text-embedding-3-small",
    input="hello",
)

image = client.images.generate(
    model="image-test",
    prompt="一张产品海报",
    n=1,
)

print(chat.id)
print(embedding.usage)
print(image.data[0].url)
```

Node.js OpenAI SDK 示例：

```ts
import OpenAI from "openai";

const client = new OpenAI({
  apiKey: "ehai_...",
  baseURL: "https://your-domain.example/v1",
});

const chat = await client.chat.completions.create({
  model: "gpt-4o-mini",
  messages: [{ role: "user", content: "hello" }],
});

const embedding = await client.embeddings.create({
  model: "text-embedding-3-small",
  input: "hello",
});

const image = await client.images.generate({
  model: "image-test",
  prompt: "一张产品海报",
  n: 1,
});

console.log(chat.id);
console.log(embedding.usage);
console.log(image.data[0]?.url);
```

错误处理建议：

- `401 unauthenticated`：AI API Key 缺失、错误、过期、吊销，或客户已停用。
- `429 rate_limited`：超过 `AI_GATEWAY_RATE_LIMIT_MAX` / `AI_GATEWAY_RATE_LIMIT_WINDOW_SECONDS`。
- `400 validation_failed`：模型不支持该接口、请求参数不合法、`stream=true` 暂不支持。
- `404 not_found`：模型未启用、渠道未启用或模型代码不存在。
- 余额不足会返回业务规则错误，客户需要先在后台充值 AI 钱包。
- 响应头 `x-entitlehub-usage-id` 可用于客户侧日志和后台调用记录排查。

## 19. API 安全注意事项

所有后台写接口必须：

- 认证。
- 权限校验。
- CSRF 校验。
- tenant_id 隔离。
- 审计。

所有客户端写接口必须：

- Bearer access token。
- session 校验。
- device 校验。
- 授权/订阅状态校验。
- 可选设备签名。
- 限流。

所有签名相关接口必须：

- 签名结果带 `kid`。
- 验签时按 `kid` 查 public key。
- key 状态为 `active` 或 `retiring` 才允许验签。
- key 状态为 `active` 才允许新签名。
- 轮换 key 时必须保留旧 public key，直到旧 token、旧更新包、旧脚本过期。

所有下载接口必须：

- 校验 token。
- 校验文件名。
- 校验授权状态。
- 设置安全响应头。
