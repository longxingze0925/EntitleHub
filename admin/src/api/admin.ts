import { apiRequest } from "./client";

export interface ListMeta {
  page: number;
  page_size: number;
}

export interface ListResponse<T> {
  items: T[];
  meta?: ListMeta;
}

export interface RoleSummary {
  id: string;
  code: string;
  name: string;
}

export interface RoleDetail extends RoleSummary {
  description?: string | null;
  builtin: boolean;
  permission_codes: string[];
  created_at: string;
  updated_at: string;
}

export interface PermissionSummary {
  code: string;
  name: string;
  resource: string;
  action: string;
}

export interface CreateRolePayload {
  code: string;
  name: string;
  description?: string;
  permission_codes: string[];
}

export interface UpdateRolePayload {
  name: string;
  description?: string;
  permission_codes: string[];
}

export interface RoleMutationResult {
  role: RoleDetail;
}

export interface RoleDeleteResult {
  deleted: boolean;
  role_id: string;
}

export interface TeamMember {
  id: string;
  email: string;
  name: string;
  phone?: string | null;
  status: string;
  email_verified: boolean;
  mfa_enabled: boolean;
  roles: RoleSummary[];
}

export interface InviteTeamMemberPayload {
  email: string;
  role_codes: string[];
}

export interface UpdateTeamMemberRolesPayload {
  role_codes: string[];
}

export interface InvitationResult {
  token: string;
  expires_at: string;
}

export interface TeamMemberMutationResult {
  member: TeamMember;
  revoked_sessions?: number;
}

export interface Customer {
  id: string;
  email: string;
  name?: string | null;
  phone?: string | null;
  company?: string | null;
  status: string;
  email_verified: boolean;
  remark?: string | null;
}

export interface CreateCustomerPayload {
  email: string;
  name?: string;
  password?: string;
  phone?: string;
  company?: string;
  remark?: string;
}

export interface UpdateCustomerPayload {
  name?: string;
  phone?: string;
  company?: string;
  remark?: string;
}

export interface CustomerMutationResult {
  customer: Customer;
  revoked_sessions?: number;
}

export interface CustomerPasswordResetResult {
  expires_at: string;
}

export interface ApplicationSummary {
  id: string;
  name: string;
  slug?: string | null;
  app_key: string;
  auth_mode: string;
  status: string;
  heartbeat_interval_seconds: number;
  offline_tolerance_seconds: number;
  max_devices_default: number;
}

export interface CreateApplicationPayload {
  name: string;
  slug?: string;
  auth_mode?: string;
  heartbeat_interval_seconds?: number;
  offline_tolerance_seconds?: number;
  max_devices_default?: number;
}

export interface UpdateApplicationPayload {
  name?: string;
  slug?: string;
  auth_mode?: string;
  status?: string;
  heartbeat_interval_seconds?: number;
  offline_tolerance_seconds?: number;
  max_devices_default?: number;
}

export interface SigningKeySummary {
  id: string;
  kid: string;
  key_scope: string;
  alg: string;
  public_key_pem: string;
  status: string;
  not_before: string;
  not_after?: string | null;
  created_at: string;
  activated_at?: string | null;
}

export interface CreateApplicationResult {
  id: string;
  app_key: string;
  app_secret: string;
  signing_key: SigningKeySummary;
  application: ApplicationSummary;
}

export interface RotateApplicationKeysResult {
  id: string;
  app_key: string;
  app_secret: string;
  signing_key: SigningKeySummary;
}

export interface RotateGlobalJwtSigningKeyResult {
  signing_key: SigningKeySummary;
  retired_key_count: number;
}

export interface AdminSessionSummary {
  id: string;
  current: boolean;
  status: string;
  ip?: string | null;
  user_agent?: string | null;
  created_at: string;
  last_seen_at?: string | null;
  expires_at: string;
  revoked_at?: string | null;
}

export interface AdminSessionRevokeResult {
  revoked: boolean;
  session_id: string;
  revoked_refresh_tokens: number;
}

export interface LicenseSummary {
  id: string;
  app_id: string;
  customer_id?: string | null;
  type: string;
  status: string;
  max_devices: number;
  starts_at?: string | null;
  expires_at?: string | null;
  revoked_at?: string | null;
}

export interface CreateLicensePayload {
  app_id: string;
  customer_id?: string;
  type?: string;
  max_devices?: number;
  starts_at?: string;
  expires_at?: string;
  features?: string[];
}

export interface CreateLicenseResult {
  license_key: string;
  license: LicenseSummary;
}

export interface LicenseMutationResult {
  license: LicenseSummary;
  revoked_sessions: number;
}

export interface SubscriptionSummary {
  id: string;
  app_id: string;
  customer_id: string;
  plan: string;
  status: string;
  max_devices: number;
  starts_at: string;
  expires_at?: string | null;
  cancelled_at?: string | null;
}

export interface CreateSubscriptionPayload {
  app_id: string;
  customer_id: string;
  plan: string;
  max_devices?: number;
  starts_at?: string;
  expires_at?: string;
  features?: string[];
}

export interface SubscriptionMutationResult {
  subscription: SubscriptionSummary;
  revoked_sessions: number;
}

export interface DeviceSummary {
  id: string;
  app_id: string;
  customer_id?: string | null;
  license_id?: string | null;
  subscription_id?: string | null;
  machine_id: string;
  device_name?: string | null;
  os?: string | null;
  app_version?: string | null;
  status: string;
  first_seen_at: string;
  last_seen_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface DeviceMutationResult {
  device: DeviceSummary;
  revoked_sessions: number;
}

export interface AuditLogSummary {
  id: string;
  actor_type: string;
  actor_id?: string | null;
  action: string;
  resource_type: string;
  resource_id?: string | null;
  ip?: string | null;
  user_agent?: string | null;
  request_id?: string | null;
  created_at: string;
}

export interface AuditLogDetail extends AuditLogSummary {
  before_json?: unknown | null;
  after_json?: unknown | null;
  metadata_json: unknown;
}

export interface AuditLogExportResult {
  items: AuditLogDetail[];
  exported_at: string;
  limit: number;
}

export interface AuditLogQueryParams {
  actor_id?: string;
  action?: string;
  resource_type?: string;
  resource_id?: string;
  start_at?: string;
  end_at?: string;
  page?: number;
  page_size?: number;
}

export interface SystemSetting {
  key: string;
  value: unknown;
  updated_at: string;
}

export interface UpdateSystemSettingPayload {
  value: unknown;
}

export interface EmailSettings {
  enabled: boolean;
  smtp_host: string;
  smtp_port: number;
  smtp_user?: string | null;
  smtp_from: string;
  smtp_password_configured: boolean;
  source: string;
  last_test_status?: string | null;
  last_test_error?: string | null;
  last_test_at?: string | null;
  updated_at?: string | null;
}

export interface UpdateEmailSettingsPayload {
  enabled: boolean;
  smtp_host: string;
  smtp_port: number;
  smtp_user?: string | null;
  smtp_from: string;
  smtp_password?: string;
  clear_password?: boolean;
}

export interface TestEmailSettingsPayload {
  to: string;
  confirm_delivery: boolean;
}

export type NotificationChannelKind = "webhook" | "email" | "pagerduty";

export interface NotificationChannel {
  id: string;
  name: string;
  kind: NotificationChannelKind;
  enabled: boolean;
  config: Record<string, unknown>;
  secret_configured: boolean;
  last_test_status?: string | null;
  last_test_error?: string | null;
  last_test_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface CreateNotificationChannelPayload {
  name: string;
  kind: NotificationChannelKind;
  enabled?: boolean;
  config: Record<string, unknown>;
  secret?: Record<string, unknown>;
}

export interface UpdateNotificationChannelPayload {
  name?: string;
  enabled?: boolean;
  config?: Record<string, unknown>;
  secret?: Record<string, unknown>;
  clear_secret?: boolean;
}

export interface TestNotificationChannelPayload {
  mode?: "dry_run" | "delivery";
  confirm_delivery?: boolean;
}

export type AiProviderKind =
  | "openai_compatible"
  | "custom_http"
  | "claude"
  | "gemini"
  | "deepseek"
  | "image"
  | "video";

export interface AiProvider {
  id: string;
  name: string;
  kind: AiProviderKind;
  base_url: string;
  enabled: boolean;
  config: Record<string, unknown>;
  secret_configured: boolean;
  created_at: string;
  updated_at: string;
}

export interface CreateAiProviderPayload {
  name: string;
  kind: AiProviderKind;
  base_url: string;
  enabled?: boolean;
  config: Record<string, unknown>;
  secret?: Record<string, unknown>;
}

export interface UpdateAiProviderPayload {
  name?: string;
  base_url?: string;
  enabled?: boolean;
  config?: Record<string, unknown>;
  secret?: Record<string, unknown>;
  clear_secret?: boolean;
}

export type AiModelModality =
  | "text"
  | "image"
  | "video"
  | "audio"
  | "embedding"
  | "multimodal";

export type AiModelBillingMode =
  | "token"
  | "per_image"
  | "video_per_second"
  | "video_per_request"
  | "audio_per_second"
  | "audio_per_minute"
  | "audio_per_request";

export interface AiModel {
  id: string;
  code: string;
  name: string;
  modality: AiModelModality;
  provider_id?: string | null;
  provider_name?: string | null;
  provider_model?: string | null;
  enabled: boolean;
  currency: string;
  billing_mode: AiModelBillingMode;
  input_1k_price_minor: number;
  output_1k_price_minor: number;
  request_price_minor: number;
  image_price_minor: number;
  second_price_minor: number;
  minute_price_minor: number;
  daily_spend_limit_minor?: number | null;
  pricing_config: Record<string, unknown>;
  metadata: Record<string, unknown>;
  created_at: string;
  updated_at: string;
}

export interface CreateAiModelPayload {
  code: string;
  name: string;
  modality: AiModelModality;
  provider_id?: string;
  provider_model?: string;
  enabled?: boolean;
  currency?: string;
  billing_mode?: AiModelBillingMode;
  input_1k_price_minor?: number;
  output_1k_price_minor?: number;
  request_price_minor?: number;
  image_price_minor?: number;
  second_price_minor?: number;
  minute_price_minor?: number;
  daily_spend_limit_minor?: number | null;
  pricing_config?: Record<string, unknown>;
  metadata?: Record<string, unknown>;
}

export interface UpdateAiModelPayload {
  name?: string;
  modality?: AiModelModality;
  provider_id?: string | null;
  provider_model?: string | null;
  enabled?: boolean;
  currency?: string;
  billing_mode?: AiModelBillingMode;
  input_1k_price_minor?: number;
  output_1k_price_minor?: number;
  request_price_minor?: number;
  image_price_minor?: number;
  second_price_minor?: number;
  minute_price_minor?: number;
  daily_spend_limit_minor?: number | null;
  pricing_config?: Record<string, unknown>;
  metadata?: Record<string, unknown>;
}

export interface AiWallet {
  customer_id: string;
  customer_email: string;
  customer_name?: string | null;
  wallet_id?: string | null;
  currency: string;
  balance_minor: number;
  held_minor: number;
  available_minor: number;
  daily_spend_limit_minor?: number | null;
  updated_at?: string | null;
}

export interface AiWalletLedgerEntry {
  id: string;
  customer_id: string;
  entry_type: string;
  amount_minor: number;
  balance_after_minor: number;
  held_after_minor: number;
  reason: string;
  reference_type?: string | null;
  reference_id?: string | null;
  metadata: Record<string, unknown>;
  created_by?: string | null;
  created_at: string;
}

export interface AdjustAiWalletPayload {
  amount_minor: number;
  reason: string;
  metadata?: Record<string, unknown>;
}

export interface UpdateAiWalletQuotaPayload {
  daily_spend_limit_minor?: number | null;
}

export interface AiApiKey {
  id: string;
  customer_id: string;
  customer_email: string;
  customer_name?: string | null;
  name: string;
  key_prefix: string;
  status: string;
  expires_at?: string | null;
  daily_spend_limit_minor?: number | null;
  last_used_at?: string | null;
  created_at: string;
  revoked_at?: string | null;
}

export interface CreateAiApiKeyPayload {
  name: string;
  expires_at?: string | null;
  daily_spend_limit_minor?: number | null;
}

export interface UpdateAiApiKeyPayload {
  name?: string;
  expires_at?: string | null;
  daily_spend_limit_minor?: number | null;
}

export interface CreateAiApiKeyResult {
  api_key: AiApiKey;
  plain_key: string;
}

export type AiAssetType = "image" | "video" | "audio" | "file";
export type AiAssetStatus = "caching" | "ready" | "failed" | "deleted";

export interface AiAsset {
  id: string;
  usage_id?: string | null;
  customer_id?: string | null;
  customer_email?: string | null;
  customer_name?: string | null;
  provider_name?: string | null;
  model_code?: string | null;
  asset_type: AiAssetType;
  status: AiAssetStatus;
  public_url?: string | null;
  mime_type?: string | null;
  file_size?: number | null;
  created_at: string;
  updated_at: string;
  deleted_at?: string | null;
  deleted_by?: string | null;
}

export interface AiUsageRecord {
  id: string;
  customer_id?: string | null;
  customer_email?: string | null;
  customer_name?: string | null;
  provider_name?: string | null;
  model_code?: string | null;
  request_id?: string | null;
  endpoint: string;
  status: string;
  provider_status?: string | null;
  provider_request_id?: string | null;
  prompt_tokens?: number | null;
  completion_tokens?: number | null;
  total_tokens?: number | null;
  charged_minor: number;
  refunded_minor: number;
  provider_cost_minor?: number | null;
  currency: string;
  price_snapshot: Record<string, unknown>;
  metadata: Record<string, unknown>;
  created_at: string;
  completed_at?: string | null;
}

export interface OutboxEventSummary {
  id: string;
  tenant_id?: string | null;
  event_type: string;
  payload: unknown;
  status: string;
  attempts: number;
  next_run_at: string;
  last_error?: string | null;
  created_at: string;
  processed_at?: string | null;
}

export interface ReleaseSummary {
  id: string;
  app_id: string;
  file_id: string;
  version: string;
  version_code: number;
  status: string;
  changelog?: string | null;
  force_update: boolean;
  published_at?: string | null;
  deprecated_at?: string | null;
  created_at: string;
  updated_at: string;
}

export interface ReleaseFileSummary {
  id: string;
  storage_key: string;
  file_name: string;
  file_size: number;
  sha256: string;
  signature_kid: string;
  signature: string;
  signature_alg: string;
  metadata: unknown;
  created_at: string;
}

export interface RegisterReleaseFilePayload {
  storage_key?: string;
  file_name: string;
  file_size: number;
  sha256: string;
}

export interface RegisterReleaseFileResult {
  file_id: string;
  file_name: string;
  file_size: number;
  sha256: string;
  signature_kid: string;
  signature: string;
  signature_alg: string;
  file: ReleaseFileSummary;
}

export interface CreateReleasePayload {
  file_id: string;
  version: string;
  version_code: number;
  changelog?: string;
  force_update?: boolean;
}

export interface SecureScriptSummary {
  id: string;
  app_id: string;
  name: string;
  version: string;
  version_code: number;
  status: string;
  content_sha256: string;
  signature_kid: string;
  signature: string;
  signature_alg: string;
  required_features: unknown[];
  expires_at?: string | null;
  published_at?: string | null;
}

export interface CreateSecureScriptPayload {
  name: string;
  version: string;
  version_code: number;
  required_features?: string[];
  expires_at?: string;
}

export interface UpdateSecureScriptContentPayload {
  content_base64: string;
  version?: string;
  version_code?: number;
}

function query(params: Record<string, string | number | boolean | undefined>): string {
  const search = new URLSearchParams();
  Object.entries(params).forEach(([key, value]) => {
    if (value !== undefined && value !== "") {
      search.set(key, String(value));
    }
  });
  const text = search.toString();

  return text ? `?${text}` : "";
}

export function listTeamMembers(params: {
  include_history?: boolean;
} = {}): Promise<{ items: TeamMember[] }> {
  return apiRequest<{ items: TeamMember[] }>(
    `/api/team/members${query({
      include_history: params.include_history
    })}`
  );
}

export function listRoles(): Promise<{ items: RoleDetail[] }> {
  return apiRequest<{ items: RoleDetail[] }>("/api/admin/roles");
}

export function listPermissions(): Promise<{ items: PermissionSummary[] }> {
  return apiRequest<{ items: PermissionSummary[] }>("/api/admin/permissions");
}

export function createRole(
  payload: CreateRolePayload
): Promise<RoleMutationResult> {
  return apiRequest<RoleMutationResult>("/api/admin/roles", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function updateRole(params: {
  id: string;
  payload: UpdateRolePayload;
}): Promise<RoleMutationResult> {
  return apiRequest<RoleMutationResult>(`/api/admin/roles/${params.id}`, {
    method: "PUT",
    body: JSON.stringify(params.payload)
  });
}

export function deleteRole(id: string): Promise<RoleDeleteResult> {
  return apiRequest<RoleDeleteResult>(`/api/admin/roles/${id}`, {
    method: "DELETE"
  });
}

export function inviteTeamMember(
  payload: InviteTeamMemberPayload
): Promise<{ member: TeamMember; invitation: InvitationResult }> {
  return apiRequest<{ member: TeamMember; invitation: InvitationResult }>(
    "/api/team/invitations",
    {
      method: "POST",
      body: JSON.stringify(payload)
    }
  );
}

export function updateTeamMemberRoles(params: {
  id: string;
  payload: UpdateTeamMemberRolesPayload;
}): Promise<TeamMemberMutationResult> {
  return apiRequest<TeamMemberMutationResult>(
    `/api/team/members/${params.id}/roles`,
    {
      method: "PUT",
      body: JSON.stringify(params.payload)
    }
  );
}

export function disableTeamMember(id: string): Promise<TeamMemberMutationResult> {
  return apiRequest<TeamMemberMutationResult>(
    `/api/team/members/${id}/disable`,
    {
      method: "POST"
    }
  );
}

export function listCustomers(params: {
  keyword?: string;
  status?: string;
  include_history?: boolean;
  page?: number;
  page_size?: number;
}): Promise<ListResponse<Customer>> {
  return apiRequest<ListResponse<Customer>>(
    `/api/admin/customers${query({
      keyword: params.keyword,
      status: params.status,
      include_history: params.include_history,
      page: params.page ?? 1,
      page_size: params.page_size ?? 20
    })}`
  );
}

export function createCustomer(
  payload: CreateCustomerPayload
): Promise<CustomerMutationResult> {
  return apiRequest<CustomerMutationResult>("/api/admin/customers", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function updateCustomer(params: {
  id: string;
  payload: UpdateCustomerPayload;
}): Promise<CustomerMutationResult> {
  return apiRequest<CustomerMutationResult>(
    `/api/admin/customers/${params.id}`,
    {
      method: "PUT",
      body: JSON.stringify(params.payload)
    }
  );
}

export function disableCustomer(id: string): Promise<CustomerMutationResult> {
  return apiRequest<CustomerMutationResult>(
    `/api/admin/customers/${id}/disable`,
    {
      method: "POST"
    }
  );
}

export function resetCustomerPassword(
  id: string
): Promise<CustomerPasswordResetResult> {
  return apiRequest<CustomerPasswordResetResult>(
    `/api/admin/customers/${id}/reset-password`,
    {
      method: "POST"
    }
  );
}

export function listApplications(params: {
  keyword?: string;
  status?: string;
  include_history?: boolean;
  page?: number;
  page_size?: number;
}): Promise<ListResponse<ApplicationSummary>> {
  return apiRequest<ListResponse<ApplicationSummary>>(
    `/api/admin/apps${query({
      keyword: params.keyword,
      status: params.status,
      include_history: params.include_history,
      page: params.page ?? 1,
      page_size: params.page_size ?? 20
    })}`
  );
}

export function createApplication(
  payload: CreateApplicationPayload
): Promise<CreateApplicationResult> {
  return apiRequest<CreateApplicationResult>("/api/admin/apps", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function updateApplication(params: {
  id: string;
  payload: UpdateApplicationPayload;
}): Promise<{ application: ApplicationSummary }> {
  return apiRequest<{ application: ApplicationSummary }>(
    `/api/admin/apps/${params.id}`,
    {
      method: "PUT",
      body: JSON.stringify(params.payload)
    }
  );
}

export function rotateApplicationKeys(
  id: string
): Promise<RotateApplicationKeysResult> {
  return apiRequest<RotateApplicationKeysResult>(
    `/api/admin/apps/${id}/rotate-keys`,
    {
      method: "POST"
    }
  );
}

export function listApplicationSigningKeys(
  id: string
): Promise<{ items: SigningKeySummary[] }> {
  return apiRequest<{ items: SigningKeySummary[] }>(
    `/api/admin/apps/${id}/signing-keys`
  );
}

export function listGlobalJwtSigningKeys(): Promise<{
  items: SigningKeySummary[];
}> {
  return apiRequest<{ items: SigningKeySummary[] }>(
    "/api/admin/security/jwt-signing-keys"
  );
}

export function rotateGlobalJwtSigningKey(): Promise<RotateGlobalJwtSigningKeyResult> {
  return apiRequest<RotateGlobalJwtSigningKeyResult>(
    "/api/admin/security/jwt-signing-keys/rotate",
    {
      method: "POST"
    }
  );
}

export function listAdminSessions(): Promise<{ items: AdminSessionSummary[] }> {
  return apiRequest<{ items: AdminSessionSummary[] }>("/api/auth/sessions");
}

export function revokeAdminSession(
  id: string
): Promise<AdminSessionRevokeResult> {
  return apiRequest<AdminSessionRevokeResult>(
    `/api/auth/sessions/${id}/revoke`,
    {
      method: "POST"
    }
  );
}

export function listLicenses(params: {
  keyword?: string;
  status?: string;
  include_history?: boolean;
  page?: number;
  page_size?: number;
}): Promise<ListResponse<LicenseSummary>> {
  return apiRequest<ListResponse<LicenseSummary>>(
    `/api/admin/licenses${query({
      keyword: params.keyword,
      status: params.status,
      include_history: params.include_history,
      page: params.page ?? 1,
      page_size: params.page_size ?? 20
    })}`
  );
}

export function createLicense(
  payload: CreateLicensePayload
): Promise<CreateLicenseResult> {
  return apiRequest<CreateLicenseResult>("/api/admin/licenses", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function revokeLicense(id: string): Promise<LicenseMutationResult> {
  return apiRequest<LicenseMutationResult>(
    `/api/admin/licenses/${id}/revoke`,
    {
      method: "POST"
    }
  );
}

export function suspendLicense(id: string): Promise<LicenseMutationResult> {
  return apiRequest<LicenseMutationResult>(
    `/api/admin/licenses/${id}/suspend`,
    {
      method: "POST"
    }
  );
}

export function renewLicense(params: {
  id: string;
  expires_at: string;
}): Promise<LicenseMutationResult> {
  return apiRequest<LicenseMutationResult>(
    `/api/admin/licenses/${params.id}/renew`,
    {
      method: "POST",
      body: JSON.stringify({ expires_at: params.expires_at })
    }
  );
}

export function resetLicenseDevices(params: {
  id: string;
  reason: string;
}): Promise<LicenseMutationResult> {
  return apiRequest<LicenseMutationResult>(
    `/api/admin/licenses/${params.id}/reset-devices`,
    {
      method: "POST",
      body: JSON.stringify({ reason: params.reason })
    }
  );
}

export function listSubscriptions(params: {
  keyword?: string;
  status?: string;
  include_history?: boolean;
  page?: number;
  page_size?: number;
}): Promise<ListResponse<SubscriptionSummary>> {
  return apiRequest<ListResponse<SubscriptionSummary>>(
    `/api/admin/subscriptions${query({
      keyword: params.keyword,
      status: params.status,
      include_history: params.include_history,
      page: params.page ?? 1,
      page_size: params.page_size ?? 20
    })}`
  );
}

export function createSubscription(
  payload: CreateSubscriptionPayload
): Promise<{ subscription: SubscriptionSummary }> {
  return apiRequest<{ subscription: SubscriptionSummary }>(
    "/api/admin/subscriptions",
    {
      method: "POST",
      body: JSON.stringify(payload)
    }
  );
}

export function cancelSubscription(
  id: string
): Promise<SubscriptionMutationResult> {
  return apiRequest<SubscriptionMutationResult>(
    `/api/admin/subscriptions/${id}/cancel`,
    {
      method: "POST"
    }
  );
}

export function listDevices(params: {
  machine_id?: string;
  status?: string;
  include_history?: boolean;
  page?: number;
  page_size?: number;
}): Promise<ListResponse<DeviceSummary>> {
  return apiRequest<ListResponse<DeviceSummary>>(
    `/api/admin/devices${query({
      machine_id: params.machine_id,
      status: params.status,
      include_history: params.include_history,
      page: params.page ?? 1,
      page_size: params.page_size ?? 20
    })}`
  );
}

export function unbindDevice(id: string): Promise<DeviceMutationResult> {
  return apiRequest<DeviceMutationResult>(`/api/admin/devices/${id}`, {
    method: "DELETE"
  });
}

export function blacklistDevice(params: {
  id: string;
  reason: string;
}): Promise<DeviceMutationResult> {
  return apiRequest<DeviceMutationResult>(
    `/api/admin/devices/${params.id}/blacklist`,
    {
      method: "POST",
      body: JSON.stringify({ reason: params.reason })
    }
  );
}

export function unblacklistDevice(id: string): Promise<DeviceMutationResult> {
  return apiRequest<DeviceMutationResult>(
    `/api/admin/devices/${id}/unblacklist`,
    {
      method: "POST"
    }
  );
}

export function listAuditLogs(
  params: AuditLogQueryParams
): Promise<ListResponse<AuditLogSummary>> {
  return apiRequest<ListResponse<AuditLogSummary>>(
    `/api/admin/audit-logs${query({
      actor_id: params.actor_id,
      action: params.action,
      resource_type: params.resource_type,
      resource_id: params.resource_id,
      start_at: params.start_at,
      end_at: params.end_at,
      page: params.page ?? 1,
      page_size: params.page_size ?? 20
    })}`
  );
}

export function getAuditLog(id: string): Promise<{ audit_log: AuditLogDetail }> {
  return apiRequest<{ audit_log: AuditLogDetail }>(
    `/api/admin/audit-logs/${id}`
  );
}

export function exportAuditLogs(
  params: Omit<AuditLogQueryParams, "page" | "page_size">
): Promise<AuditLogExportResult> {
  return apiRequest<AuditLogExportResult>(
    `/api/admin/audit-logs/export${query({
      actor_id: params.actor_id,
      action: params.action,
      resource_type: params.resource_type,
      resource_id: params.resource_id,
      start_at: params.start_at,
      end_at: params.end_at
    })}`
  );
}

export function listSystemSettings(): Promise<{ items: SystemSetting[] }> {
  return apiRequest<{ items: SystemSetting[] }>("/api/admin/system/settings");
}

export function updateSystemSetting(params: {
  key: string;
  payload: UpdateSystemSettingPayload;
}): Promise<{ setting: SystemSetting }> {
  return apiRequest<{ setting: SystemSetting }>(
    `/api/admin/system/settings/${encodeURIComponent(params.key)}`,
    {
      method: "PUT",
      body: JSON.stringify(params.payload)
    }
  );
}

export function getEmailSettings(): Promise<EmailSettings> {
  return apiRequest<EmailSettings>("/api/admin/system/email-settings");
}

export function updateEmailSettings(
  payload: UpdateEmailSettingsPayload
): Promise<EmailSettings> {
  return apiRequest<EmailSettings>("/api/admin/system/email-settings", {
    method: "PUT",
    body: JSON.stringify(payload)
  });
}

export function testEmailSettings(
  payload: TestEmailSettingsPayload
): Promise<EmailSettings> {
  return apiRequest<EmailSettings>("/api/admin/system/email-settings/test", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function listNotificationChannels(params: {
  include_history?: boolean;
} = {}): Promise<{
  items: NotificationChannel[];
}> {
  return apiRequest<{ items: NotificationChannel[] }>(
    `/api/admin/notification-channels${query({
      include_history: params.include_history
    })}`
  );
}

export function createNotificationChannel(
  payload: CreateNotificationChannelPayload
): Promise<{ channel: NotificationChannel }> {
  return apiRequest<{ channel: NotificationChannel }>(
    "/api/admin/notification-channels",
    {
      method: "POST",
      body: JSON.stringify(payload)
    }
  );
}

export function updateNotificationChannel(params: {
  id: string;
  payload: UpdateNotificationChannelPayload;
}): Promise<{ channel: NotificationChannel }> {
  return apiRequest<{ channel: NotificationChannel }>(
    `/api/admin/notification-channels/${params.id}`,
    {
      method: "PUT",
      body: JSON.stringify(params.payload)
    }
  );
}

export function testNotificationChannel(
  id: string,
  payload: TestNotificationChannelPayload = {}
): Promise<{ channel: NotificationChannel }> {
  return apiRequest<{ channel: NotificationChannel }>(
    `/api/admin/notification-channels/${id}/test`,
    {
      method: "POST",
      body: JSON.stringify(payload)
    }
  );
}

export function listAiProviders(params: {
  include_history?: boolean;
} = {}): Promise<{ items: AiProvider[] }> {
  return apiRequest<{ items: AiProvider[] }>(
    `/api/admin/ai/providers${query({
      include_history: params.include_history
    })}`
  );
}

export function createAiProvider(
  payload: CreateAiProviderPayload
): Promise<{ provider: AiProvider }> {
  return apiRequest<{ provider: AiProvider }>("/api/admin/ai/providers", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function updateAiProvider(params: {
  id: string;
  payload: UpdateAiProviderPayload;
}): Promise<{ provider: AiProvider }> {
  return apiRequest<{ provider: AiProvider }>(
    `/api/admin/ai/providers/${params.id}`,
    {
      method: "PUT",
      body: JSON.stringify(params.payload)
    }
  );
}

export function listAiModels(params: {
  include_history?: boolean;
  modality?: string;
} = {}): Promise<{ items: AiModel[] }> {
  return apiRequest<{ items: AiModel[] }>(
    `/api/admin/ai/models${query({
      include_history: params.include_history,
      modality: params.modality
    })}`
  );
}

export function createAiModel(
  payload: CreateAiModelPayload
): Promise<{ model: AiModel }> {
  return apiRequest<{ model: AiModel }>("/api/admin/ai/models", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function updateAiModel(params: {
  id: string;
  payload: UpdateAiModelPayload;
}): Promise<{ model: AiModel }> {
  return apiRequest<{ model: AiModel }>(`/api/admin/ai/models/${params.id}`, {
    method: "PUT",
    body: JSON.stringify(params.payload)
  });
}

export function listAiWallets(params: {
  include_history?: boolean;
} = {}): Promise<{ items: AiWallet[] }> {
  return apiRequest<{ items: AiWallet[] }>(
    `/api/admin/ai/wallets${query({
      include_history: params.include_history
    })}`
  );
}

export function adjustAiWallet(params: {
  customerId: string;
  payload: AdjustAiWalletPayload;
}): Promise<{ wallet: AiWallet; ledger_entry?: AiWalletLedgerEntry | null }> {
  return apiRequest<{ wallet: AiWallet; ledger_entry?: AiWalletLedgerEntry | null }>(
    `/api/admin/ai/customers/${params.customerId}/wallet/adjust`,
    {
      method: "POST",
      body: JSON.stringify(params.payload)
    }
  );
}

export function updateAiWalletQuota(params: {
  customerId: string;
  payload: UpdateAiWalletQuotaPayload;
}): Promise<{ wallet: AiWallet; ledger_entry?: AiWalletLedgerEntry | null }> {
  return apiRequest<{ wallet: AiWallet; ledger_entry?: AiWalletLedgerEntry | null }>(
    `/api/admin/ai/customers/${params.customerId}/wallet/quota`,
    {
      method: "PUT",
      body: JSON.stringify(params.payload)
    }
  );
}

export function listAiWalletLedger(params: {
  customerId: string;
  page?: number;
  page_size?: number;
}): Promise<ListResponse<AiWalletLedgerEntry>> {
  return apiRequest<ListResponse<AiWalletLedgerEntry>>(
    `/api/admin/ai/customers/${params.customerId}/wallet/ledger${query({
      page: params.page ?? 1,
      page_size: params.page_size ?? 20
    })}`
  );
}

export function listAiApiKeys(params: {
  include_history?: boolean;
  customer_id?: string;
} = {}): Promise<{ items: AiApiKey[] }> {
  return apiRequest<{ items: AiApiKey[] }>(
    `/api/admin/ai/api-keys${query({
      include_history: params.include_history,
      customer_id: params.customer_id
    })}`
  );
}

export function createAiApiKey(params: {
  customerId: string;
  payload: CreateAiApiKeyPayload;
}): Promise<CreateAiApiKeyResult> {
  return apiRequest<CreateAiApiKeyResult>(
    `/api/admin/ai/customers/${params.customerId}/api-keys`,
    {
      method: "POST",
      body: JSON.stringify(params.payload)
    }
  );
}

export function updateAiApiKey(params: {
  id: string;
  payload: UpdateAiApiKeyPayload;
}): Promise<{ api_key: AiApiKey }> {
  return apiRequest<{ api_key: AiApiKey }>(
    `/api/admin/ai/api-keys/${params.id}`,
    {
      method: "PUT",
      body: JSON.stringify(params.payload)
    }
  );
}

export function revokeAiApiKey(id: string): Promise<{ api_key: AiApiKey }> {
  return apiRequest<{ api_key: AiApiKey }>(
    `/api/admin/ai/api-keys/${id}/revoke`,
    {
      method: "POST"
    }
  );
}

export function listAiAssets(params: {
  status?: string;
  asset_type?: string;
  customer_id?: string;
  page?: number;
  page_size?: number;
} = {}): Promise<ListResponse<AiAsset>> {
  return apiRequest<ListResponse<AiAsset>>(
    `/api/admin/ai/assets${query({
      status: params.status,
      asset_type: params.asset_type,
      customer_id: params.customer_id,
      page: params.page ?? 1,
      page_size: params.page_size ?? 50
    })}`
  );
}

export function deleteAiAsset(id: string): Promise<{ asset: AiAsset }> {
  return apiRequest<{ asset: AiAsset }>(`/api/admin/ai/assets/${id}`, {
    method: "DELETE"
  });
}

export function listAiUsageRecords(params: {
  status?: string;
  customer_id?: string;
  page?: number;
  page_size?: number;
} = {}): Promise<ListResponse<AiUsageRecord>> {
  return apiRequest<ListResponse<AiUsageRecord>>(
    `/api/admin/ai/usage-records${query({
      status: params.status,
      customer_id: params.customer_id,
      page: params.page ?? 1,
      page_size: params.page_size ?? 50
    })}`
  );
}

export function listOutboxEvents(params: {
  status?: string;
  event_type?: string;
  page?: number;
  page_size?: number;
}): Promise<ListResponse<OutboxEventSummary>> {
  return apiRequest<ListResponse<OutboxEventSummary>>(
    `/api/admin/outbox-events${query({
      status: params.status,
      event_type: params.event_type,
      page: params.page ?? 1,
      page_size: params.page_size ?? 20
    })}`
  );
}

export function retryOutboxEvent(id: string): Promise<{ event: OutboxEventSummary }> {
  return apiRequest<{ event: OutboxEventSummary }>(
    `/api/admin/outbox-events/${id}/retry`,
    {
      method: "POST"
    }
  );
}

export function listReleases(params: {
  appId: string;
  status?: string;
  include_history?: boolean;
  page?: number;
  page_size?: number;
}): Promise<ListResponse<ReleaseSummary>> {
  return apiRequest<ListResponse<ReleaseSummary>>(
    `/api/admin/apps/${params.appId}/releases${query({
      status: params.status,
      include_history: params.include_history,
      page: params.page ?? 1,
      page_size: params.page_size ?? 20
    })}`
  );
}

export function registerReleaseFile(params: {
  appId: string;
  payload: RegisterReleaseFilePayload;
}): Promise<RegisterReleaseFileResult> {
  return apiRequest<RegisterReleaseFileResult>(
    `/api/admin/apps/${params.appId}/release-files`,
    {
      method: "POST",
      body: JSON.stringify(params.payload)
    }
  );
}

export function uploadReleaseFile(params: {
  appId: string;
  file: File;
}): Promise<RegisterReleaseFileResult> {
  const fileName = encodeURIComponent(params.file.name);

  return apiRequest<RegisterReleaseFileResult>(
    `/api/admin/apps/${params.appId}/release-files/upload?file_name=${fileName}`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/octet-stream"
      },
      body: params.file
    }
  );
}

export function createRelease(params: {
  appId: string;
  payload: CreateReleasePayload;
}): Promise<{ release: ReleaseSummary }> {
  return apiRequest<{ release: ReleaseSummary }>(
    `/api/admin/apps/${params.appId}/releases`,
    {
      method: "POST",
      body: JSON.stringify(params.payload)
    }
  );
}

export function publishRelease(id: string): Promise<{ release: ReleaseSummary }> {
  return apiRequest<{ release: ReleaseSummary }>(
    `/api/admin/releases/${id}/publish`,
    {
      method: "POST"
    }
  );
}

export function deprecateRelease(id: string): Promise<{ release: ReleaseSummary }> {
  return apiRequest<{ release: ReleaseSummary }>(
    `/api/admin/releases/${id}/deprecate`,
    {
      method: "POST"
    }
  );
}

export function listSecureScripts(params: {
  appId: string;
  status?: string;
  include_history?: boolean;
  page?: number;
  page_size?: number;
}): Promise<ListResponse<SecureScriptSummary>> {
  return apiRequest<ListResponse<SecureScriptSummary>>(
    `/api/admin/apps/${params.appId}/secure-scripts${query({
      status: params.status,
      include_history: params.include_history,
      page: params.page ?? 1,
      page_size: params.page_size ?? 20
    })}`
  );
}

export function createSecureScript(params: {
  appId: string;
  payload: CreateSecureScriptPayload;
}): Promise<{ script: SecureScriptSummary }> {
  return apiRequest<{ script: SecureScriptSummary }>(
    `/api/admin/apps/${params.appId}/secure-scripts`,
    {
      method: "POST",
      body: JSON.stringify(params.payload)
    }
  );
}

export function updateSecureScriptContent(params: {
  id: string;
  payload: UpdateSecureScriptContentPayload;
}): Promise<{ script: SecureScriptSummary }> {
  return apiRequest<{ script: SecureScriptSummary }>(
    `/api/admin/secure-scripts/${params.id}/content`,
    {
      method: "POST",
      body: JSON.stringify(params.payload)
    }
  );
}

export function publishSecureScript(
  id: string
): Promise<{ script: SecureScriptSummary }> {
  return apiRequest<{ script: SecureScriptSummary }>(
    `/api/admin/secure-scripts/${id}/publish`,
    {
      method: "POST"
    }
  );
}

export function deprecateSecureScript(
  id: string
): Promise<{ script: SecureScriptSummary }> {
  return apiRequest<{ script: SecureScriptSummary }>(
    `/api/admin/secure-scripts/${id}/deprecate`,
    {
      method: "POST"
    }
  );
}
