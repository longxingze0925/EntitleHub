# 客户端 SDK 设计文档

本文档定义软件客户端 SDK 的设计。SDK 用于让软件客户端安全接入后台系统，完成激活、登录、授权验证、心跳、更新下载、脚本拉取和本地缓存。

SDK 的核心目标：

- 简化客户端接入。
- 隐藏复杂认证和刷新逻辑。
- 防止 token 滥用。
- 支持设备绑定。
- 支持请求签名和防重放。
- 支持更新包和脚本验签。
- 支持本地缓存和离线容忍。

## 1. SDK 基本原则

客户端不可信。

SDK 不能假设：

- 本地时间可信。
- machine_id 绝对可信。
- 本地缓存不会被复制。
- app_secret 不会被逆向。
- 用户不能修改客户端代码。

SDK 的作用：

- 提高攻击成本。
- 降低接入错误。
- 标准化请求签名。
- 标准化 token 刷新。
- 标准化更新验签。

真正的授权判断必须在服务端完成。

## 2. 第一版支持语言

建议第一版先做一个 SDK。

推荐顺序：

```text
1. Rust SDK
2. Go SDK
3. Python SDK
4. .NET SDK
```

如果客户端主要是 Windows 桌面软件，可以优先做：

```text
.NET SDK 或 Rust SDK
```

如果系统后端是 Rust，优先做 Rust SDK 可以复用签名、hash、错误类型设计。

## 3. SDK 包结构

Rust SDK 建议结构：

```text
sdk-rust/
  src/
    lib.rs
    client.rs
    config.rs
    error.rs
    session.rs
    device.rs
    signing.rs
    response.rs
    cache.rs
    update.rs
    script.rs
    http.rs
```

Go SDK 建议结构：

```text
sdk-go/
  client.go
  config.go
  error.go
  session.go
  device.go
  signing.go
  cache.go
  update.go
  script.go
```

## 4. SDK 初始化

配置结构：

```text
base_url
app_key
server_public_key
cache_dir
enable_cache
enable_request_signing
timeout
retry_count
```

示例：

```rust
let client = LicenseClient::new(ClientConfig {
    base_url: "https://api.example.com".into(),
    app_key: "app_xxx".into(),
    server_public_key: "...".into(),
    cache_dir: None,
    enable_cache: true,
    enable_request_signing: true,
});
```

## 5. 设备 ID 设计

SDK 需要生成 machine_id。

machine_id 来源可以组合：

- OS machine id。
- 主板序列号。
- CPU 信息。
- 磁盘序列号。
- 用户目录 hash。

注意：

- 不要上传原始硬件敏感信息。
- 只上传 hash 后的 machine_id。
- machine_id 可能变化，服务端要允许管理员解绑。

machine_id 示例：

```text
sha256(app_key + normalized_machine_fingerprint)
```

Rust SDK 提供 `device::machine_id_from_fingerprint_parts(app_key, parts)`：

- `parts` 由调用方采集，SDK 不主动读取硬件信息。
- 每个片段会 trim、压缩空白、小写化。
- 片段会排序去重，避免采集顺序影响结果。
- 输出为 SHA-256 的 base64url no-pad 文本，只上传 hash 后的 machine_id。

Rust SDK 提供 `device::normalize_machine_id(machine_id)`，用于读取缓存后做非空和 trim 校验。

Rust SDK 提供 `device::DeviceIdentity`：

- `DeviceIdentity::generate(app_key, fingerprint_parts)` 会同时生成 machine_id 和 Ed25519 设备 keypair。
- `DeviceIdentity::rotate_key()` 保留 machine_id，生成新的 Ed25519 设备 keypair。
- `device_public_key` 可直接放入激活或登录请求。
- `private_key_pkcs8_base64` 是私钥材料的文本载荷，只能交给安全存储或加密文件保存。
- `private_key_pkcs8_der()` 用于发起设备签名请求前还原私钥字节。

Rust SDK 提供认证请求构造 helper：

- `auth::build_activation_request(input)` 生成 `/api/client/auth/activate` 请求体。
- `auth::build_customer_login_request(input)` 生成 `/api/client/auth/login` 请求体。
- 两个 helper 都会复用 `DeviceIdentity.machine_id` 和 `DeviceIdentity.device_public_key`，并 trim 必填字段。

Rust SDK 提供 `auth::ClientBootstrap`：

- 持有 `DeviceIdentity` 和 `SessionManager`。
- 可生成激活/登录请求体。
- `apply_auth_response_json(json, now_unix)` 可解析认证响应并初始化 `SessionManager`。
- 不负责实际 HTTP 请求，也不负责 session 或私钥落盘。

## 6. 设备密钥设计

首次激活时，SDK 生成设备密钥对。

推荐算法：

```text
Ed25519
```

Rust SDK 提供 `signing::generate_device_keypair()`：

- 返回 `public_key_pem`、`public_key_base64` 和 `private_key_pkcs8_der`。
- `device_public_key` 推荐上传 `public_key_pem`。
- `private_key_pkcs8_der` 必须由调用方放进系统安全存储或加密文件。
- `device::build_rotate_device_key_request(device)` 生成 `/api/client/devices/self/rotate-key` 请求体。
- `device::RotateDeviceKeyResponse::from_json(json)` 解析裸 `data` 响应。
- `device::RotateDeviceKeyResponse::from_api_response_json(json)` 解析完整后端 `ApiResponse` 包装，并返回新 `device_key_id`。

本地保存：

```text
device_private_key
device_public_key
device_key_id
device_id
session_id
```

服务端保存：

```text
device_public_key
```

后续请求使用 private key 签名。

设备 key 轮换顺序：

```text
1. 基于当前 DeviceIdentity 调用 rotate_key() 生成 next_device
2. 用 next_device 构造 rotate-key 请求体
3. 用旧 device key 和旧 device_key_id 对 rotate-key 请求签名
4. 服务端返回新的 device_key_id
5. 本地保存 next_device 和新的 device_key_id
```

## 7. 本地密钥存储

优先级：

```text
Windows: Credential Manager 或 DPAPI
macOS: Keychain
Linux: Secret Service/libsecret
fallback: 本地加密文件
```

fallback 加密文件方案：

```text
key = HKDF(machine_id + app_key)
算法 = AES-256-GCM
```

注意：

- 这不是绝对安全，只是提高复制成本。
- logout 时必须清理本地缓存。
- refresh token 必须尽量放在系统安全存储中。

## 8. 激活流程

方法：

```text
activate(license_key)
```

流程：

```text
1. 生成 machine_id
2. 如果没有 device keypair，生成 keypair
3. 请求 /api/client/auth/activate
4. 服务端返回 access_token、refresh_token、device_id、session_id
5. SDK 保存 session
6. SDK 保存授权信息缓存
```

请求：

```json
{
  "app_key": "app_xxx",
  "license_key": "AAAA-BBBB-CCCC",
  "machine_id": "hash",
  "device_name": "Windows PC",
  "os": "windows",
  "app_version": "1.0.0",
  "device_public_key": "base64"
}
```

响应：

```json
{
  "access_token": "...",
  "refresh_token": "...",
  "expires_in": 900,
  "refresh_expires_in": 2592000,
  "session_id": "uuid",
  "device_id": "uuid",
  "features": ["pro"]
}
```

`access_token` 是 EdDSA JWT，Header 包含 `kid`。Rust SDK 提供 `access_token::verify_access_token` 和 `access_token::verify_access_token_with_jwks_refresh` 用于本地检查 token 形态和过期时间，可从 `/.well-known/jwks.json` 获取全局 `jwt_access_token` 公钥验签；真正授权状态仍必须以服务端接口返回为准。

## 9. 客户账号登录流程

方法：

```text
login(email, password)
```

流程：

```text
1. 生成 machine_id
2. 生成或读取 device keypair
3. 请求 /api/client/auth/login
4. 服务端校验客户账号；有效订阅只决定功能权限，不阻止登录
5. 返回 session 和当前 entitlement 状态
6. SDK 保存 session
```

密码处理：

- 第一版建议直接通过 HTTPS 传输密码。
- 不要做自定义弱加密。
- 如果做客户端预哈希，服务端仍必须 Argon2id 存储最终 hash。

## 10. access token 刷新

SDK 必须自动刷新 access token。

触发条件：

- access token 过期。
- access token 距离过期小于 60 秒。
- 服务端返回 401 且错误类型可刷新。

流程：

```text
1. 读取 refresh_token
2. 请求 /api/client/auth/refresh
3. 服务端返回新 access_token 和新 refresh_token
4. SDK 替换本地 session
5. 重试原请求一次
```

注意：

- refresh 请求本身不能无限重试。
- refresh 失败必须清理本地 session。
- 并发请求同时刷新时必须加锁，避免多个请求复用旧 refresh token。

Rust SDK 提供 `session::ClientSessionState`、`session::SessionInit` 和 `session::SessionRefresh`：

- 根据 `expires_in`、`refresh_expires_in` 和当前时间计算本地过期时间。
- `needs_access_token_refresh(now_unix, 60)` 用于提前 60 秒刷新。
- `apply_refresh` 必须同时替换 access token 和 refresh token，避免旧 refresh token 被复用。
- `to_json` / `from_json` 只负责状态序列化和形态校验；refresh token 落盘必须由调用方放进系统安全存储或加密文件。
- `ClientAuthSessionResponse::from_json` 可解析裸 `data` 响应；`ClientAuthSessionResponse::from_api_response_json` 可解析完整后端 `ApiResponse` 包装。
- 认证响应会保留登录/激活响应里的可选 `device_key_id`；登录/激活后用 `ClientSessionState::from_auth_response`，刷新后用 `into_session_refresh`。
- 认证响应会解析可选 `subscription_id`、`entitlement_id`、`entitlement_kind`、`entitlement_status` 和 `entitlement_active`；无订阅登录时这些字段允许为空或 inactive，SDK 不应把无订阅登录当成认证失败。

Rust SDK 提供 `response::parse_api_response_data<T>(json)`：

- 后端统一响应形态为 `{ code, message, data, request_id }`。
- `code == 0` 时返回 `data` 和 `request_id`。
- `code != 0` 时返回 `SdkError::ApiError(code, message)`。
- `data` 缺失或 JSON 形态错误时返回 `SdkError::InvalidApiResponse`。

## 11. 并发刷新控制

SDK 必须保证同一时刻只有一个 refresh 在执行。

错误做法：

```text
多个请求同时发现 token 快过期，同时调用 refresh。
```

这会导致 refresh token 被复用，服务端可能撤销 session。

正确做法：

```text
使用 mutex / singleflight
第一个请求执行 refresh
其他请求等待 refresh 完成
```

Rust SDK 的 `session::SessionManager` 使用互斥锁保护本地 session：

- `authorization_header_value(now_unix, refresh_before_seconds, refresh_fn)` 会在 token 快过期时执行 refresh。
- 并发调用时只有第一个线程会执行 `refresh_fn`，其他线程等待并读取刷新后的 token。
- refresh 失败或 refresh token 已过期时，SDK 清理本地 session。
- `refresh_fn` 由上层提供，负责调用 `/api/client/auth/refresh` 并返回 `SessionRefresh`。

## 12. 请求签名

保护接口请求头：

```text
Authorization: Bearer <access_token>
X-Device-Id: <device_id>
X-Device-Key-Id: <device_key_id>
X-Timestamp: <unix_seconds>
X-Nonce: <random>
X-Body-SHA256: <hash>
X-Signature: <signature>
```

`X-Nonce` 使用 16–128 个 URL-safe 随机字符：`A-Z`、`a-z`、`0-9`、`-`、`_`。

Rust SDK 提供 `signing::generate_device_nonce()`，默认生成 24 字节随机数并使用 base64url no-pad 编码，输出 32 个 URL-safe 字符。

签名 payload：

```text
method + "\n" +
path + "\n" +
body_sha256 + "\n" +
timestamp + "\n" +
nonce + "\n" +
device_id + "\n" +
device_key_id + "\n" +
session_id
```

签名算法：

```text
Ed25519
```

SDK 必须为这些接口签名：

- heartbeat
- verify
- update check
- script fetch
- sync push
- device unbind

Rust SDK 提供纯签名 helper：

```text
sign_device_request(input) -> DeviceSignatureHeaders
```

返回字段可直接映射到请求头：

```text
X-Device-Id
X-Device-Key-Id
X-Timestamp
X-Nonce
X-Body-SHA256
X-Signature
```

Rust SDK 还提供请求组装 helper：

```text
build_authorized_device_request(input, refresh_fn) -> AuthorizedRequestParts
build_authorized_cached_device_request(cache, session_manager, input, refresh_fn) -> AuthorizedRequestParts
```

要求：

- 输入 method、path、body、timestamp、nonce、device id、device key id、设备私钥和 `SessionManager`。
- 缓存版 helper 从 `SdkCacheEnvelope` 读取 `DeviceIdentity` 和 `device_key_id`，从 `SessionManager` 读取刷新后的 `device_id` 与 `session_id`。
- 输出 `Authorization`、`X-Device-Id`、`X-Device-Key-Id`、`X-Timestamp`、`X-Nonce`、`X-Body-SHA256`、`X-Signature` 请求头。
- 不绑定具体 HTTP 库；调用方负责把 headers 写入实际请求。
- 如果 access token 快过期，先通过 `SessionManager` 执行 refresh，再使用刷新后的 session 签名。

## 13. 心跳

方法：

```text
heartbeat()
```

接口：

```text
POST /api/client/auth/heartbeat
```

作用：

- 更新设备 last_seen_at。
- 发现授权失效。
- 发现设备被拉黑。
- 同步服务器时间。

响应：

```json
{
  "status": "ok",
  "server_time": 1710000000,
  "license_status": "active",
  "entitlement_id": "uuid",
  "entitlement_kind": "subscription",
  "entitlement_status": "active",
  "entitlement_active": true,
  "subscription_id": "uuid"
}
```

Rust SDK 提供 `auth::HeartbeatResponse::from_json(json)` 解析裸 `data` 响应，`auth::HeartbeatResponse::from_api_response_json(json)` 解析完整后端 `ApiResponse` 包装。

## 14. 验证授权

方法：

```text
verify()
```

接口：

```text
POST /api/client/auth/verify
```

响应：

```json
{
  "valid": true,
  "features": ["pro"],
  "expires_at": "2027-01-01T00:00:00Z",
  "entitlement_id": "uuid",
  "entitlement_kind": "subscription",
  "entitlement_status": "active",
  "entitlement_active": true,
  "subscription_id": "uuid"
}
```

Rust SDK 提供 `auth::VerifyResponse::from_json(json)` 解析裸 `data` 响应，`auth::VerifyResponse::from_api_response_json(json)` 解析完整后端 `ApiResponse` 包装。

SDK 行为：

- valid=false 时返回明确错误。
- `entitlement_active=false` 时保留登录态，但禁用需要订阅/授权的功能。
- 客户端 AI API 由服务端实时强制有效订阅、AI 钱包状态、余额和限额。
- 授权过期时标记本地功能不可用，不应仅因为无订阅就清理登录 session。
- 网络失败时可按 offline_tolerance 使用缓存。

## 15. 离线容忍

SDK 可以支持离线容忍，但必须有限制。

缓存内容：

```text
last_success_verify_at
license_expires_at
features
offline_tolerance_seconds
signature
```

离线可用条件：

- 本地缓存未过期。
- 当前时间没有明显回拨。
- 距离 last_success_verify_at 未超过 offline_tolerance。
- 本地缓存签名有效。

注意：

- 本地缓存只能延缓网络失败影响。
- 不能让已过期授权永久可用。

## 16. 更新检查

方法：

```text
check_update()
```

接口：

```text
GET /api/client/releases/latest
```

响应：

```json
{
  "app_id": "uuid",
  "version": "1.0.1",
  "version_code": 101,
  "download_url": "...",
  "file_size": 123,
  "sha256": "...",
  "published_at_unix": 1780000000,
  "signature_kid": "release-2026-06-01",
  "signature": "...",
  "signature_alg": "Ed25519",
  "force_update": false
}
```

Rust SDK 提供 `update::UpdateInfo::from_json(json)` 解析裸 `data` 响应，`update::UpdateInfo::from_api_response_json(json)` 解析完整后端 `ApiResponse` 包装。两个入口都会执行基础字段校验。

SDK 判断：

- `version_code` 大于当前版本才提示更新。
- `force_update=true` 时业务层可强制更新。
- 低于当前版本默认拒绝，防回滚。
- Rust SDK 提供 `ensure_update_not_downgrade(update, current_version_code)`，并提供带当前版本参数的下载/验证包装函数。
- Rust SDK 验证的是 release 元数据签名，`app_id`、`version`、`version_code`、`sha256`、`file_size`、`published_at_unix` 必须和服务端响应一致。

## 17. 更新下载

方法：

```text
download_update(update_info, target_path)
```

流程：

```text
1. 请求 download_url
2. 流式写入临时文件
3. 计算 sha256
4. 对比服务端 sha256
5. 验证服务端签名
6. 原子移动到目标路径
```

禁止：

- 直接覆盖原文件。
- hash 未验证就安装。
- 签名未验证就安装。
- 忽略 signature_alg。

## 18. 更新元数据签名验证

签名 payload 必须和服务端一致。

更新检查响应里的 `signature_*` 是 release 元数据签名，payload 固定为：

```text
app_id
version
version_code
sha256
file_size
published_at_unix
```

字段按上面的顺序使用 `\n` 拼接。文件登记接口返回的文件签名可用于后台审计和文件登记校验，但 SDK 安装更新时不得用文件签名替代 release 元数据签名。

## 18.1 JWKS 公钥缓存

SDK 必须支持按 `kid` 获取验签公钥。

接口：

```text
GET /api/client/apps/{app_key}/jwks
```

缓存规则：

```text
1. SDK 初始化后可懒加载 JWKS
2. 验签时先按 signature_kid 查本地缓存
3. 找不到 kid 时刷新 JWKS
4. 刷新后仍找不到 kid，拒绝安装或拒绝使用脚本
5. 缓存 public key，不缓存 private key
```

Rust SDK 提供 `jwks::JwksCache`、`jwks::parse_jwks_json` 和 `jwks::require_eddsa_public_key`。`JwksCache::require_eddsa_public_key_with_refresh` 支持在缺少 `kid` 时调用业务传入的 HTTP 拉取闭包，并把返回的 JWKS 响应体 upsert 到缓存。

更新包和脚本模块提供带 JWKS 刷新的验签包装函数，调用方传入 `JwksCache` 和拉取闭包即可在缺少 `kid` 时刷新后重试。

更新包和脚本响应必须包含：

```text
signature
signature_alg
signature_kid
```

注意：

- `signature_kid` 不能忽略。
- key 轮换期间，SDK 需要同时接受旧 `kid` 和新 `kid`。
- 如果服务端返回的 key 已过期或被撤销，SDK 不能继续使用。
- JWKS 请求失败时，可以使用未过期缓存；缓存过期且无法刷新时，禁止安装新更新包和新脚本。

## 19. 脚本拉取

方法：

```text
fetch_script(script_id)
```

接口：

```text
POST /api/client/secure-scripts/fetch
```

Rust SDK 提供 `script::ScriptPackage::from_json(json)` 解析裸 `data` 响应，`script::ScriptPackage::from_api_response_json(json)` 解析完整后端 `ApiResponse` 包装。两个入口都会执行基础字段校验。

SDK 行为：

```text
1. 请求脚本包
2. 验证签名
3. 验证 hash
4. 检查 expires_at
5. base64 解码内容
6. 返回给业务层
```

脚本执行不应由 SDK 自动完成。SDK 只负责安全拉取、base64 解码和校验。SDK 第一版不负责脚本端到端解密。

## 20. SDK 错误类型

必须定义稳定错误：

```text
NetworkError
Unauthorized
Forbidden
LicenseExpired
LicenseRevoked
DeviceBlacklisted
SessionExpired
RefreshFailed
SignatureInvalid
HashMismatch
ReplayRejected
ServerError
InvalidResponse
ApiError
InvalidApiResponse
CacheError
```

业务层可以根据错误类型做处理。

## 21. 重试策略

允许重试：

- 网络超时。
- 502。
- 503。
- 504。

不允许重试：

- 400。
- 401 refresh 失败。
- 403。
- 授权过期。
- 设备拉黑。
- 签名失败。

重试必须指数退避。

## 22. 日志规则

SDK 日志禁止输出：

- access_token
- refresh_token
- private_key
- license_key
- app_secret
- full machine fingerprint

可以输出：

- request_id
- endpoint
- status code
- error type

## 23. 缓存文件结构

缓存建议：

```json
{
  "version": 1,
  "app_key": "...",
  "device_id": "...",
  "device_key_id": "...",
  "machine_id": "...",
  "session_id": "...",
  "access_token": "...",
  "refresh_token": "...",
  "access_expires_at": 1710000000,
  "refresh_expires_at": 1710000000,
  "features": [],
  "jwks_cache": {
    "fetched_at": 1710000000,
    "expires_at": 1710086400,
    "keys": []
  },
  "last_success_verify_at": 1710000000
}
```

该 JSON 必须加密后落盘。

Rust SDK 当前提供 `ClientSessionState::to_json` / `from_json` 作为缓存载荷，不直接写文件，不直接承诺 OS 安全存储。上层集成必须在落盘前加密，并确保日志不会输出 token 字段。

Rust SDK 提供 `cache::SdkCacheEnvelope`：

- `version` 当前为 `1`。
- 包含 `app_key`、`DeviceIdentity`、可选 `device_key_id`、可选 `ClientSessionState`、JWKS keys 和 `saved_at_unix`。
- `new_with_device_key_id(...)` 可在激活/登录成功后把服务端返回的 key id 一起放入缓存。
- `apply_device_key_rotation(device, device_key_id, saved_at_unix)` 用于设备 key 轮换成功后原子更新缓存里的 `DeviceIdentity` 和 `device_key_id`。
- `to_json` / `from_json` 只做 JSON 序列化和形态校验。
- `jwks_cache()` 可还原 `JwksCache`。
- `session_manager()` 可还原 `SessionManager`。
- 带 `device_key_id` 的缓存可直接配合 `request::build_authorized_cached_device_request(...)` 生成带认证和设备签名的请求头。
- SDK 不直接负责加密或落盘；调用方必须在保存前加密。
- `into_logout_cache(options, saved_at_unix)` 用于本地 logout 清理；默认清除 session、保留 device identity 和 JWKS keys。
- 如果 `keep_device_identity = false`，返回 `None`，调用方应删除整个缓存文件或安全存储记录。

## 24. logout

方法：

```text
logout()
```

流程：

```text
1. 调用 /api/client/auth/logout
2. 清理本地 session
3. 清理 access_token
4. 清理 refresh_token
5. 保留或删除 device key 根据配置决定
```

默认保留 device key，避免下次同设备重新生成导致设备异常。

Rust SDK 本地清理 helper：

- `auth::LogoutResponse::from_json(json)` 解析裸 `data` 响应，`auth::LogoutResponse::from_api_response_json(json)` 解析完整后端 `ApiResponse` 包装。
- `auth::ClientBootstrap::clear_session()` 清理内存中的 session。
- `cache::SdkCacheEnvelope::clear_session_for_logout(saved_at_unix)` 原地清理缓存 session。
- `cache::SdkCacheEnvelope::into_logout_cache(options, saved_at_unix)` 根据策略生成 logout 后缓存。

## 25. 设备密钥轮换

方法：

```text
rotate_device_key()
```

接口：

```text
POST /api/client/devices/self/rotate-key
```

流程：

```text
1. 生成新的 DeviceIdentity，保留原 machine_id
2. 用新 public key 构造请求体
3. 用旧 private key + 旧 device_key_id 签名请求
4. 服务端把旧 key 标记 rotated，并返回新 device_key_id
5. SDK 通过 SdkCacheEnvelope::apply_device_key_rotation 保存新 DeviceIdentity 和新 device_key_id
```

轮换不清理 session。请求失败时必须保留旧 key，不能提前覆盖本地缓存。

## 26. 解绑设备

方法：

```text
unbind_device(password)
```

流程：

```text
1. 调用 /api/client/devices/self
2. 服务端撤销 session
3. SDK 清理本地 session
4. 可选择清理 device key
```

订阅账号模式下建议要求重新输入密码。

## 27. SDK 公共 API 建议

最小 API：

```text
new(config)
activate(license_key)
login(email, password)
logout()
verify()
heartbeat()
check_update(current_version_code)
download_update(update_info, path)
fetch_script(script_id)
rotate_device_key()
unbind_device(password)
is_activated()
current_features()
```

## 28. 测试要求

SDK 必须测试：

- 激活成功。
- 激活失败。
- access token 自动刷新。
- refresh token 失败清缓存。
- 并发 refresh 只发生一次。
- 请求签名正确。
- 设备 key 轮换请求体和响应解析正确。
- 后端 `ApiResponse` 包装解析正确。
- heartbeat / verify / logout 响应解析正确。
- 推荐调用流程可以串起认证响应、缓存、签名请求、响应解析、设备 key 轮换和 logout 清理。
- body 被篡改验签失败。
- 下载 hash mismatch。
- 下载签名失败。
- 缓存加密解密。
- logout 清缓存。
- 离线容忍。

## 29. 示例调用

伪代码：

```text
client = LicenseClient::new(config)

if !client.is_activated() {
    client.activate(license_key)
}

client.verify()
client.heartbeat()

update = client.check_update(current_version_code)
if update.available {
    client.download_update(update, path)
}
```

## 30. 安全注意事项

必须记住：

- SDK 不能替代服务端授权判断。
- 客户端私钥可能被盗，但比 app_secret 更强。
- app_secret 不能作为唯一安全边界。
- machine_id 不能作为唯一安全边界。
- refresh token 泄露后必须能通过轮换和复用检测发现。
- 下载文件必须 hash + signature 双校验。
- 脚本必须签名后再交给业务执行。

## 31. 第一版 SDK 完成标准

第一版完成标准：

- 可以激活授权码。
- 可以保存加密 session。
- 可以自动 refresh。
- 可以发送带签名请求。
- 可以 heartbeat。
- 可以 verify。
- 可以 check update。
- 可以下载更新并验签。
- 可以 logout。
- 有基础测试。
