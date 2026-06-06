# AI 开发任务拆分

本文档用于把后台系统拆分成 AI 可执行任务。每个任务必须小而完整，完成后可以运行测试或手动验证。

开发原则：

- 先做最小闭环，再扩展功能。
- 每次只改一个模块。
- 所有接口必须有权限和租户隔离。
- 所有数据库变更必须有 migration。
- 所有安全逻辑必须有测试。
- 不允许为了赶进度跳过 refresh token 轮换、审计和限流。

## 阶段 0：项目初始化

### 任务 0.1 创建后端项目

目标：

- 创建 Rust 后端项目。
- 引入 Axum、sqlx、PostgreSQL、Redis、tracing、serde。
- 建立基础目录结构。

验收：

- `cargo build` 成功。
- `/health` 返回 ok。
- 配置可以从环境变量读取。

AI 提示词：

```text
创建 Rust Axum 后端项目，按开发手册目录结构组织代码，加入配置加载、健康检查、统一错误类型和 tracing 日志。不要实现业务功能。
```

### 任务 0.2 创建数据库迁移框架

目标：

- 配置 sqlx migration。
- 建立数据库连接池。
- 加入启动时连接检查。

验收：

- 可以运行 migration。
- 数据库不可用时启动失败并输出明确错误。

### 任务 0.3 创建前端项目

目标：

- 创建 React + TypeScript 管理后台。
- 引入 Ant Design。
- 建立登录页和基础布局。

验收：

- `npm run build` 成功。
- 可以访问登录页。

## 阶段 1：身份与租户

### 任务 1.1 创建 tenants 表和模型

目标：

- 添加 `tenants` migration。
- 实现租户 model/repository。

验收：

- 可以创建租户。
- slug 唯一。
- soft delete 生效。

### 任务 1.2 创建 team_members 表和密码哈希

目标：

- 添加 `team_members` migration。
- 实现 Argon2id 密码哈希。
- 实现密码校验。

验收：

- 密码不明文存储。
- 单元测试覆盖正确密码和错误密码。

### 任务 1.2.1 创建一次性 token 和 MFA 恢复码表

目标：

- 添加 `one_time_tokens` migration。
- 添加 `admin_mfa_recovery_codes` migration。
- 实现 token hash、过期、使用、撤销逻辑。
- 实现 recovery code hash、使用、重新生成逻辑。
- 实现邮箱验证请求和确认接口。

验收：

- 邀请 token、密码重置 token、邮箱验证 token 都只存 hash。
- token 使用后不能再次使用。
- recovery code 只展示一次，数据库不保存明文。
- 过期 token 和已撤销 token 会被拒绝。
- 邮箱验证成功后正确更新 `email_verified`。

### 任务 1.3 初始化 owner

目标：

- 提供命令或接口初始化第一个租户和 owner。
- 初始密码必须随机生成或来自环境变量。

验收：

- 不允许使用固定默认密码。
- 如果生成随机密码，只在初始化日志中输出一次。

### 任务 1.4 后台登录

目标：

- 实现 `/api/auth/login`。
- 校验邮箱、密码、账号状态、租户状态。
- 创建 admin_session。
- 设置 HttpOnly Cookie。

验收：

- 登录成功返回当前用户信息。
- 登录失败不会泄露账号是否存在。
- Cookie 包含 HttpOnly、Secure、SameSite。

### 任务 1.5 后台 session 中间件

目标：

- 从 Cookie 读取 session。
- 校验 session 未过期、未撤销。
- 加载 team_member 和 tenant。
- 注入请求上下文。

验收：

- 未登录访问受保护接口返回 401。
- 被撤销 session 返回 401。
- 被禁用用户返回 403。

### 任务 1.6 后台登出

目标：

- 实现 `/api/auth/logout`。
- 撤销当前 session。
- 清除 Cookie。

验收：

- 登出后原 Cookie 不可访问受保护接口。

## 阶段 2：权限系统

### 任务 2.1 创建角色权限表

目标：

- 创建 roles、permissions、role_permissions、team_member_roles。
- 初始化内置权限和内置角色。

验收：

- owner 拥有所有权限。
- viewer 只有只读权限。

### 任务 2.2 权限中间件

目标：

- 实现 permission guard。
- 后端接口可声明所需权限。

验收：

- 无权限返回 403。
- 前端隐藏菜单不影响后端权限。

### 任务 2.3 团队成员管理

接口：

- `GET /api/team/members`
- `POST /api/team/invitations`
- `PUT /api/team/members/{id}/roles`
- `POST /api/team/members/{id}/disable`

验收：

- 不能禁用最后一个 owner。
- 禁用成员后撤销其 session。
- 记录审计。

## 阶段 3：客户与应用

### 任务 3.1 客户表和后台接口

目标：

- 实现 customers 表。
- 实现客户 CRUD。

验收：

- 同一租户 email 唯一。
- 不同租户可使用相同 email。
- 查询必须带 tenant_id。

### 任务 3.2 应用表和密钥生成

目标：

- 实现 applications 表。
- 创建应用时生成 app_key、app_secret、签名密钥对。

验收：

- app_secret 只展示一次。
- 数据库存 app_secret_hash。
- private_key 加密存储。

### 任务 3.3 应用管理接口

接口：

- `GET /api/admin/apps`
- `POST /api/admin/apps`
- `GET /api/admin/apps/{id}`
- `PUT /api/admin/apps/{id}`
- `POST /api/admin/apps/{id}/rotate-keys`

验收：

- 非本租户应用不可访问。
- rotate key 记录审计。

## 阶段 4：授权码

### 任务 4.1 licenses 表

目标：

- 创建 licenses 表。
- 授权码生成和 hash 存储。

验收：

- 授权码只展示一次。
- 数据库不存明文。

### 任务 4.2 授权后台接口

接口：

- `GET /api/admin/licenses`
- `POST /api/admin/licenses`
- `POST /api/admin/licenses/{id}/revoke`
- `POST /api/admin/licenses/{id}/suspend`
- `POST /api/admin/licenses/{id}/renew`
- `POST /api/admin/licenses/{id}/reset-devices`

验收：

- 吊销授权后相关 client_session 被撤销。
- 续期更新 expires_at。
- reset-devices 撤销设备 session。

### 任务 4.3 授权有效性判断

目标：

- 实现 license service。
- 判断 active、未过期、未吊销、未暂停。

验收：

- 过期授权不可激活。
- 暂停授权不可激活。
- 吊销授权不可激活。

## 阶段 5：设备和客户端 session

### 任务 5.1 devices 表

目标：

- 创建 devices 表。
- 实现设备绑定和设备数量限制。

验收：

- 超过 max_devices 拒绝激活。
- 同一 app + machine_id 不重复创建设备。

### 任务 5.2 客户端激活接口

接口：

```text
POST /api/client/auth/activate
```

目标：

- 校验 app_key。
- 校验 license_key。
- 校验 machine_id。
- 创建设备。
- 创建 client_session。
- 返回 access_token 和 refresh_token。

验收：

- 成功激活返回 token。
- 设备超限返回错误。
- 拉黑设备不能激活。

### 任务 5.3 client_sessions 和 refresh token

目标：

- 创建 client_sessions。
- 创建 client_refresh_tokens。
- 实现 access token 签发。
- 实现 refresh token 轮换。

验收：

- access token 15 分钟过期。
- refresh token 存 hash。
- refresh 成功后旧 refresh token 失效。
- 旧 refresh token 复用撤销 session。

### 任务 5.4 客户端认证中间件

目标：

- 验证 access token。
- 校验 session、device、license/subscription 状态。

验收：

- 被吊销 session 不能访问。
- 被拉黑设备不能访问。
- 授权过期不能访问。

### 任务 5.5 心跳和验证接口

接口：

- `POST /api/client/auth/heartbeat`
- `POST /api/client/auth/verify`

验收：

- heartbeat 更新 last_seen_at。
- verify 返回授权状态和 features。

## 阶段 6：设备密钥和请求签名

### 任务 6.1 device_keys 表

目标：

- 存储设备 public key。
- 存储 `tenant_id`、`app_id`、`device_id`。
- 支持 key rotation。

验收：

- 激活时保存 public key。
- `X-Device-Key-Id` 能定位到正确 active key。
- 禁用 device key 后签名失败。

### 任务 6.2 请求签名中间件

目标：

- 校验 X-Timestamp。
- 校验 X-Nonce。
- 校验 X-Body-SHA256。
- 校验 X-Signature。

验收：

- 过期 timestamp 拒绝。
- 重复 nonce 拒绝。
- 篡改 body 拒绝。
- 伪造签名拒绝。

### 任务 6.3 Redis nonce 存储

目标：

- nonce 存 Redis。
- TTL 5 分钟。

验收：

- 多实例下重复 nonce 也会失败。

### 任务 6.4 signing_keys 和 JWKS

目标：

- 创建 `signing_keys` 表。
- 实现签名密钥生成、加密保存、轮换、撤销。
- 实现 `GET /.well-known/jwks.json`。
- 实现 `GET /api/client/apps/{app_key}/jwks`。

验收：

- JWKS 只返回 public key。
- 每个 key 都有稳定 `kid`。
- 新签名只能使用 `active` key。
- 验签允许 `active` 和 `retiring` key。
- 旧 key 在旧 token、旧更新包、旧脚本过期前不能删除。

## 阶段 7：版本分发

### 任务 7.1 release_files 表和上传

目标：

- 支持版本文件上传。
- 流式保存到对象存储。
- 计算 sha256。
- 服务端签名。

验收：

- 大文件不整包读内存。
- 文件 hash 正确。
- signature 可被 SDK 验证。

### 任务 7.2 releases 表和发布

目标：

- 创建 release。
- 发布 release。
- 获取 latest release。

验收：

- latest 返回最高 version_code 的 published release。
- deprecated release 不返回。

### 任务 7.3 短时下载 token

目标：

- 生成短时下载 token。
- token 绑定 file_id、device_id、app_id。

验收：

- token 过期不能下载。
- token 和文件不匹配不能下载。
- 文件名路径穿越失败。

### 任务 7.4 SDK 下载验签

目标：

- SDK 下载文件后验证 sha256。
- 验证服务端签名。

验收：

- 文件被篡改时 SDK 报错。
- 签名缺失时 SDK 报错。

## 阶段 8：脚本下发

### 任务 8.1 secure_scripts 表

目标：

- 保存脚本元数据。
- 保存加密内容、hash、签名。

验收：

- draft 脚本不能被客户端拉取。
- published 脚本可以拉取。

### 任务 8.2 脚本发布接口

接口：

- `POST /api/admin/apps/{app_id}/secure-scripts`
- `POST /api/admin/secure-scripts/{id}/content`
- `POST /api/admin/secure-scripts/{id}/publish`
- `POST /api/admin/secure-scripts/{id}/deprecate`

验收：

- 发布记录审计。
- 修改内容后重置为 draft。

### 任务 8.3 客户端拉取脚本

接口：

- `GET /api/client/secure-scripts/versions`
- `POST /api/client/secure-scripts/fetch`

验收：

- 无 required feature 不能拉取。
- 拉取内容必须带签名。

## 阶段 9：审计与限流

### 任务 9.1 审计日志中间件

目标：

- 记录后台关键写操作。
- 敏感字段脱敏。

验收：

- password、token、secret 不出现在审计日志中。
- 创建/修改/删除关键资源有审计。

### 任务 9.2 Redis 限流

目标：

- 实现分布式限流。
- 不使用内存限流作为生产方案。

验收：

- 登录限流生效。
- 客户端激活限流生效。
- refresh 限流生效。

## 阶段 10：前端后台

### 任务 10.1 登录页

要求：

- 不保存 token 到 localStorage。
- 登录后进入后台。
- 401 自动跳登录。

### 任务 10.2 基础布局和权限菜单

要求：

- 根据后端返回 permissions 显示菜单。
- 前端隐藏只做体验，不做安全边界。

### 任务 10.3 模块页面

按顺序实现：

1. 仪表盘。
2. 团队成员。
3. 客户管理。
4. 应用管理。
5. 授权管理。
6. 设备管理。
7. 版本管理。
8. 脚本管理。
9. 审计日志。

## 阶段 11：生产加固

### 任务 11.1 安全头

目标：

- HSTS。
- CSP。
- X-Frame-Options。
- Referrer-Policy。

验收：

- CSP 不包含 unsafe-eval。

### 任务 11.2 备份恢复

目标：

- PostgreSQL 定时备份。
- MinIO/S3 文件备份。
- 恢复脚本。

验收：

- 可以在测试环境恢复。

### 任务 11.3 监控

目标：

- 指标。
- 结构化日志。
- request_id。
- 错误追踪。

验收：

- 能查看接口耗时。
- 能查看错误率。
- 能查看登录失败和限流次数。

## AI 每次开发输出模板

AI 每次完成任务必须输出：

```text
任务编号：
完成内容：
修改文件：
新增 migration：
新增接口：
安全检查：
测试命令：
测试结果：
剩余风险：
下一步建议：
```

## AI 禁止行为

禁止：

- 跳过 migration。
- 跳过 tenant_id。
- 在前端保存 token。
- 明文存密码。
- 明文存 refresh token。
- 明文存 app_secret。
- 写死密钥。
- 删除无关代码。
- 顺手重构无关模块。
- 没有测试就说安全完成。

## 优先级建议

最高优先级：

```text
租户隔离
后台 session
密码安全
权限控制
client refresh token 轮换
设备撤销
审计脱敏
```

中优先级：

```text
设备签名
热更新签名
脚本加密
Redis 分布式限流
```

后续优先级：

```text
支付
复杂报表
多语言 SDK
OIDC
SSO
```
