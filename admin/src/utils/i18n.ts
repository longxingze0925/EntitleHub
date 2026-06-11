import { ApiError } from "../api/client";

const statusText: Record<string, string> = {
  active: "启用",
  archived: "已归档",
  blacklisted: "已拉黑",
  both: "授权码和订阅",
  builtin: "内置",
  cancelled: "已取消",
  custom: "自定义",
  deprecated: "已废弃",
  disabled: "禁用",
  draft: "草稿",
  email: "邮件",
  enabled: "已启用",
  enterprise: "企业版",
  expired: "已过期",
  failed: "失败",
  force: "强制",
  license: "授权码",
  missing: "未配置",
  normal: "普通",
  not_started: "未开始",
  pagerduty: "PagerDuty",
  past_due: "逾期",
  pending: "待处理",
  processing: "处理中",
  processed: "已处理",
  published: "已发布",
  revoked: "已吊销",
  set: "已配置",
  standard: "标准版",
  subscription: "订阅",
  success: "成功",
  suspended: "已暂停",
  trial: "试用版",
  trialing: "试用中",
  unbound: "已解绑",
  untested: "未测试",
  webhook: "Webhook"
};

const messageText: Record<string, string> = {
  application_create_failed: "应用创建失败",
  application_created: "应用已创建",
  application_keys_rotated: "应用密钥已轮换",
  application_required: "请选择应用",
  application_rotate_keys_failed: "应用密钥轮换失败",
  application_update_failed: "应用更新失败",
  application_updated: "应用已更新",
  applications_load_failed: "应用列表加载失败",
  account_disabled: "账号已被禁用",
  activation_rate_limited: "激活请求过于频繁，请稍后再试",
  already_revoked: "已被吊销",
  app_disabled: "应用已被禁用",
  app_not_found: "应用不存在",
  audit_log_detail_failed: "审计详情加载失败",
  audit_logs_export_failed: "审计日志导出失败",
  audit_logs_load_failed: "审计日志加载失败",
  business_rule_failed: "业务规则校验失败",
  conflict: "数据冲突，请刷新后重试",
  customer_create_failed: "客户创建失败",
  customer_created: "客户已创建",
  customer_disable_failed: "客户禁用失败",
  customer_disabled: "客户已禁用，已撤销 {count} 个会话",
  customer_password_reset_email_queued: "密码重置邮件已加入发送队列",
  customer_password_reset_failed: "密码重置失败",
  customer_update_failed: "客户更新失败",
  customer_updated: "客户已更新",
  customers_load_failed: "客户列表加载失败",
  device_blacklisted: "设备已拉黑",
  device_limit_exceeded: "设备数量已达到上限",
  device_not_activated: "设备尚未激活",
  device_not_found: "设备不存在",
  device_status_update_failed: "设备状态更新失败",
  device_unblacklisted: "设备已解除拉黑",
  device_unbound: "设备已解绑",
  devices_load_failed: "设备列表加载失败",
  duplicate_email: "邮箱已存在",
  email_verified: "邮箱已验证",
  email_verify_requested: "验证邮件已发送",
  forbidden: "无权限执行此操作",
  internal_error: "服务内部错误",
  invalid_credentials: "邮箱或密码不正确",
  invalid_license_state: "授权状态不允许执行此操作",
  invalid_release_state: "版本状态不允许执行此操作",
  invalid_request: "请求参数不正确",
  invalid_script_state: "脚本状态不允许执行此操作",
  invite_token_invalid: "邀请令牌无效或已过期",
  license_create_failed: "授权创建失败",
  license_created: "授权已创建",
  license_devices_reset: "授权设备已重置，已撤销 {count} 个会话",
  license_renewed: "授权已续期",
  license_revoked: "授权已吊销，已撤销 {count} 个会话",
  license_status_update_failed: "授权状态更新失败",
  license_suspended: "授权已暂停，已撤销 {count} 个会话",
  license_expired: "授权已过期",
  license_invalid: "授权无效",
  license_not_found: "授权不存在",
  licenses_load_failed: "授权列表加载失败",
  ai_wallet_ledger_load_failed: "计费流水加载失败",
  jwt_key_rotated: "JWT 密钥已轮换，已停用 {count} 个旧密钥",
  member_disabled: "成员已禁用，已撤销 {count} 个会话",
  member_invited: "成员已邀请",
  member_roles_updated: "成员角色已更新",
  mfa_disabled: "多因素认证已关闭",
  mfa_enabled: "多因素认证已启用",
  mfa_already_enabled: "多因素认证已启用",
  mfa_failed: "多因素验证码不正确",
  mfa_not_enabled: "多因素认证未启用",
  mfa_required: "需要多因素验证码",
  notification_channel_save_failed: "通知渠道保存失败",
  notification_channel_saved: "通知渠道已保存",
  notification_channel_name_exists: "通知渠道名称已存在",
  notification_channel_test_failed: "通知渠道测试失败",
  notification_channel_test_passed: "通知渠道配置校验通过",
  notification_channel_test_sent: "测试消息已发送",
  notification_channels_load_failed: "通知渠道加载失败",
  outbox_event_retry_failed: "任务重试失败",
  outbox_event_retry_scheduled: "任务已安排重试",
  outbox_events_load_failed: "任务队列加载失败",
  password_changed: "密码已修改，已撤销 {count} 个会话",
  password_reset_confirmed: "密码已重置",
  password_reset_requested: "重置邮件已发送，请查看邮箱",
  password_reset_rate_limited: "密码重置请求过于频繁，请稍后再试",
  password_reset_token_invalid: "密码重置令牌无效或已过期",
  permissions_load_failed: "权限列表加载失败",
  rate_limited: "请求过于频繁，请稍后再试",
  recovery_codes_only_shown_once: "恢复码只会显示一次，请立即保存",
  recovery_codes_regenerated: "恢复码已重新生成",
  release_create_failed: "版本创建失败",
  release_created: "版本已创建",
  release_delete_failed: "版本删除失败",
  release_deleted: "版本已删除",
  release_deprecated: "版本已废弃",
  release_detail_failed: "版本详情加载失败",
  release_file_required: "请选择版本文件",
  release_published: "版本已发布",
  release_status_update_failed: "版本状态更新失败",
  release_update_failed: "版本更新失败",
  release_updated: "版本已更新",
  release_not_found: "版本不存在",
  refresh_rate_limited: "会话刷新过于频繁，请稍后再试",
  refresh_reuse_detected: "检测到会话令牌复用，请重新登录",
  releases_load_failed: "版本列表加载失败",
  role_create_failed: "角色创建失败",
  role_created: "角色已创建",
  role_delete_failed: "角色删除失败",
  role_deleted: "角色已删除",
  role_update_failed: "角色更新失败",
  role_updated: "角色已更新",
  roles_load_failed: "角色列表加载失败",
  secure_script_content_update_failed: "脚本内容更新失败",
  secure_script_content_updated: "脚本内容已更新",
  secure_script_create_failed: "脚本创建失败",
  secure_script_created: "脚本已创建",
  secure_script_deprecated: "脚本已废弃",
  secure_script_published: "脚本已发布",
  secure_script_required: "请选择脚本",
  secure_script_status_update_failed: "脚本状态更新失败",
  script_not_found: "脚本不存在",
  secure_scripts_load_failed: "脚本列表加载失败",
  service_unavailable: "服务暂不可用，请稍后重试",
  session_expired: "登录已过期，请重新登录",
  signature_invalid: "签名无效",
  signature_required: "缺少签名",
  subscription_cancel_failed: "订阅取消失败",
  subscription_cancelled: "订阅已取消，已撤销 {count} 个会话",
  subscription_create_failed: "订阅创建失败",
  subscription_created: "订阅已创建",
  subscription_devices_reset: "订阅设备已重置，已撤销 {count} 个会话",
  subscription_inactive: "订阅不可用",
  subscription_renewed: "订阅已续期",
  subscription_resumed: "订阅已恢复",
  subscription_status_update_failed: "订阅状态更新失败",
  subscription_suspended: "订阅已暂停，已撤销 {count} 个会话",
  subscriptions_load_failed: "订阅列表加载失败",
  system_setting_save_failed: "系统配置保存失败",
  system_setting_saved: "系统配置已保存",
  system_settings_load_failed: "系统配置加载失败",
  team_member_disable_failed: "成员禁用失败",
  team_member_invite_failed: "成员邀请失败",
  team_member_roles_update_failed: "成员角色更新失败",
  team_invitation_accepted: "团队邀请已接受，请登录",
  team_members_load_failed: "团队成员加载失败",
  tenant_forbidden: "无权访问该租户",
  tenant_not_found: "租户不存在",
  token_expired: "令牌已过期",
  token_invalid: "令牌无效",
  unauthenticated: "请先登录",
  user_not_found: "用户不存在",
  validation_failed: "参数校验失败",
  weak_password: "密码强度不足"
};

const roleText: Record<string, { name: string; description: string }> = {
  owner: {
    name: "所有者",
    description: "拥有租户全部权限"
  },
  admin: {
    name: "管理员",
    description: "拥有大部分管理权限"
  },
  developer: {
    name: "开发者",
    description: "负责应用、版本、脚本和设备相关操作"
  },
  viewer: {
    name: "查看者",
    description: "只读查看租户数据"
  }
};

const resourceText: Record<string, string> = {
  ai: "AI",
  app: "应用",
  audit: "审计",
  customer: "客户",
  device: "设备",
  license: "授权",
  member: "成员",
  notification: "通知渠道",
  permission: "权限",
  release: "版本",
  role: "角色",
  script: "脚本",
  security: "安全",
  subscription: "订阅",
  system: "系统",
  tenant: "租户"
};

const permissionText: Record<string, string> = {
  "ai:api_key:update": "管理 AI API Key",
  "ai:asset:delete": "删除 AI 生成素材",
  "ai:model:update": "管理 AI 模型价格",
  "ai:provider:update": "管理 AI 渠道",
  "ai:read": "查看接口计费",
  "ai:wallet:update": "调整 AI 钱包余额",
  "app:create": "创建应用",
  "app:delete": "删除应用",
  "app:read": "查看应用",
  "app:read_key": "查看应用公钥",
  "app:rotate_key": "轮换应用密钥",
  "app:update": "更新应用",
  "audit:export": "导出审计日志",
  "audit:read": "查看审计日志",
  "customer:create": "创建客户",
  "customer:delete": "删除客户",
  "customer:disable": "禁用客户",
  "customer:enable": "启用客户",
  "customer:read": "查看客户",
  "customer:reset_password": "重置客户密码",
  "customer:update": "更新客户",
  "device:blacklist": "拉黑设备",
  "device:read": "查看设备",
  "device:revoke_session": "撤销设备会话",
  "device:unbind": "解绑设备",
  "device:unblacklist": "解除设备拉黑",
  "device:update": "更新设备",
  "license:create": "创建授权",
  "license:read": "查看授权",
  "license:renew": "续期授权",
  "license:reset_device": "重置授权设备",
  "license:revoke": "吊销授权",
  "license:suspend": "暂停授权",
  "license:update": "更新授权",
  "member:delete": "删除团队成员",
  "member:disable": "禁用团队成员",
  "member:enable": "启用团队成员",
  "member:invite": "邀请团队成员",
  "member:read": "查看团队成员",
  "member:reset_password": "重置团队成员密码",
  "member:update": "更新团队成员",
  "notification:read": "查看通知渠道",
  "notification:update": "更新通知渠道",
  "permission:read": "查看权限",
  "release:create": "创建版本",
  "release:delete": "删除版本",
  "release:deprecate": "废弃版本",
  "release:publish": "发布版本",
  "release:read": "查看版本",
  "release:update": "更新版本",
  "release:upload": "上传版本文件",
  "role:create": "创建角色",
  "role:delete": "删除角色",
  "role:read": "查看角色",
  "role:update": "更新角色",
  "script:create": "创建脚本",
  "script:delete": "删除脚本",
  "script:deprecate": "废弃脚本",
  "script:publish": "发布脚本",
  "script:read": "查看脚本",
  "script:revoke": "撤销脚本",
  "script:update": "更新脚本",
  "security:read": "查看安全设置",
  "security:retry_event": "重试任务",
  "security:revoke_session": "撤销会话",
  "security:rotate_key": "轮换安全密钥",
  "security:view_events": "查看安全事件",
  "server_api_key:read": "查看服务端 Key",
  "server_api_key:update": "管理服务端 Key",
  "subscription:cancel": "取消订阅",
  "subscription:create": "创建订阅",
  "subscription:read": "查看订阅",
  "subscription:renew": "续期订阅",
  "subscription:reset_device": "重置订阅设备",
  "subscription:resume": "恢复订阅",
  "subscription:suspend": "暂停订阅",
  "subscription:update": "更新订阅",
  "system:read": "查看系统配置",
  "system:update": "更新系统配置",
  "tenant:delete": "删除租户",
  "tenant:read": "查看租户",
  "tenant:update": "更新租户"
};

const outboxEventTypeText: Record<string, string> = {
  "email.admin_password_reset": "管理员密码重置邮件",
  "email.customer_email_verify": "客户邮箱验证邮件",
  "email.customer_password_reset": "客户密码重置邮件",
  "email.team_invite": "团队邀请邮件",
  "email.team_member_email_verify": "管理员邮箱验证邮件"
};

interface RoleLabelInput {
  code: string;
  name?: string | null;
  description?: string | null;
}

interface PermissionLabelInput {
  code: string;
  name?: string | null;
  resource?: string | null;
}

export function tStatus(value?: string | null): string {
  if (!value) {
    return "-";
  }

  return statusText[value] ?? value;
}

export function tMessage(value?: string | null): string {
  if (!value) {
    return "";
  }

  const separatorIndex = value.indexOf(":");
  const key = separatorIndex >= 0 ? value.slice(0, separatorIndex) : value;
  const detail = separatorIndex >= 0 ? value.slice(separatorIndex + 1) : "";
  const translated = messageText[key] ?? statusText[key] ?? value;

  return detail ? translated.replace("{count}", detail) : translated;
}

export function tApiError(error: unknown): string | null {
  if (!error) {
    return null;
  }

  return error instanceof ApiError
    ? tMessage(error.message)
    : tMessage("service_unavailable");
}

export function tOption(value: string) {
  return {
    value,
    label: tStatus(value)
  };
}

interface TemporalStatusInput {
  status?: string | null;
  starts_at?: string | null;
  expires_at?: string | null;
}

const temporalActiveStatuses = new Set(["active", "trialing"]);

export function effectiveTemporalStatus(
  input: TemporalStatusInput,
  now: Date = new Date()
): string | null | undefined {
  const { status } = input;

  if (!status || !temporalActiveStatuses.has(status)) {
    return status;
  }

  const nowMs = now.getTime();
  const startsAtMs = parseDateMs(input.starts_at);
  if (startsAtMs !== undefined && startsAtMs > nowMs) {
    return "not_started";
  }

  const expiresAtMs = parseDateMs(input.expires_at);
  if (expiresAtMs !== undefined && expiresAtMs <= nowMs) {
    return "expired";
  }

  return status;
}

function parseDateMs(value?: string | null): number | undefined {
  if (!value) {
    return undefined;
  }

  const ms = Date.parse(value);
  return Number.isFinite(ms) ? ms : undefined;
}

export function tRoleName(
  code?: string | null,
  fallbackName?: string | null
): string {
  if (!code) {
    return fallbackName ?? "";
  }

  return roleText[code]?.name ?? fallbackName ?? code;
}

export function tRoleDescription(
  code?: string | null,
  fallbackDescription?: string | null
): string {
  if (!code) {
    return fallbackDescription ?? "";
  }

  return roleText[code]?.description ?? fallbackDescription ?? "";
}

export function tRoleLabel(
  role: RoleLabelInput,
  options: { includeCode?: boolean } = {}
): string {
  const name = tRoleName(role.code, role.name);
  return options.includeCode ? `${name}（${role.code}）` : name;
}

export function tResource(value?: string | null): string {
  if (!value) {
    return "-";
  }

  return resourceText[value] ?? value;
}

export function tPermissionName(permission: PermissionLabelInput): string {
  return permissionText[permission.code] ?? permission.name ?? permission.code;
}

export function tPermissionLabel(
  permission: PermissionLabelInput,
  options: { includeCode?: boolean } = {}
): string {
  const name = tPermissionName(permission);
  return options.includeCode ? `${name}（${permission.code}）` : name;
}

export function tOutboxEventType(
  value?: string | null,
  options: { includeCode?: boolean } = {}
): string {
  if (!value) {
    return "-";
  }

  const label = outboxEventTypeText[value] ?? value;
  return options.includeCode && label !== value ? `${label}（${value}）` : label;
}
