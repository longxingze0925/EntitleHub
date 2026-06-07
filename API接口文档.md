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
- `/metrics` 输出 Prometheus text 格式的 HTTP、依赖、worker 和通知投递指标。

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
- 客户存在有效订阅。
- 设备数量未超限。

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

## 18. API 安全注意事项

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
