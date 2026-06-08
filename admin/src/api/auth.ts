import { apiRequest } from "./client";
import type { AuthProfile, LoginResponse } from "../types/api";

export interface LoginPayload {
  email: string;
  password: string;
  mfa_code?: string;
}

export function login(payload: LoginPayload): Promise<LoginResponse> {
  return apiRequest<LoginResponse>("/api/auth/login", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function me(): Promise<AuthProfile> {
  return apiRequest<AuthProfile>("/api/auth/me");
}

export function logout(): Promise<Record<string, never>> {
  return apiRequest<Record<string, never>>("/api/auth/logout", {
    method: "POST",
    body: JSON.stringify({})
  });
}

export interface ChangePasswordPayload {
  old_password: string;
  new_password: string;
}

export interface ChangePasswordResult {
  ok: boolean;
  revoked_sessions: number;
}

export function changePassword(
  payload: ChangePasswordPayload
): Promise<ChangePasswordResult> {
  return apiRequest<ChangePasswordResult>("/api/auth/password", {
    method: "PUT",
    body: JSON.stringify(payload)
  });
}

export function requestEmailVerify(): Promise<{ expires_at: string }> {
  return apiRequest<{ expires_at: string }>("/api/auth/email/verify/request", {
    method: "POST",
    body: JSON.stringify({})
  });
}

export function confirmEmailVerify(token: string): Promise<{
  team_member_id: string;
  email_verified: boolean;
}> {
  return apiRequest<{
    team_member_id: string;
    email_verified: boolean;
  }>("/api/auth/email/verify/confirm", {
    method: "POST",
    body: JSON.stringify({ token })
  });
}

export function requestPasswordReset(email: string): Promise<{ ok: boolean }> {
  return apiRequest<{ ok: boolean }>("/api/auth/password/reset/request", {
    method: "POST",
    body: JSON.stringify({ email })
  });
}

export function confirmPasswordReset(payload: {
  token: string;
  new_password: string;
}): Promise<{
  ok: boolean;
  revoked_sessions: number;
}> {
  return apiRequest<{
    ok: boolean;
    revoked_sessions: number;
  }>("/api/auth/password/reset/confirm", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export interface AcceptTeamInvitationPayload {
  token: string;
  name: string;
  password: string;
}

export interface AcceptedTeamMember {
  id: string;
  tenant_id: string;
  email: string;
  name: string;
  status: string;
  email_verified: boolean;
}

export function acceptTeamInvitation(
  payload: AcceptTeamInvitationPayload
): Promise<{ member: AcceptedTeamMember }> {
  return apiRequest<{ member: AcceptedTeamMember }>(
    "/api/team/invitations/accept",
    {
      method: "POST",
      body: JSON.stringify(payload)
    }
  );
}

export function confirmClientEmailVerify(token: string): Promise<{
  customer_id: string;
  email_verified: boolean;
}> {
  return apiRequest<{
    customer_id: string;
    email_verified: boolean;
  }>("/api/client/auth/email/verify/confirm", {
    method: "POST",
    body: JSON.stringify({ token })
  });
}

export function confirmClientPasswordReset(payload: {
  token: string;
  new_password: string;
}): Promise<{
  ok: boolean;
  revoked_sessions: number;
  revoked_refresh_tokens: number;
}> {
  return apiRequest<{
    ok: boolean;
    revoked_sessions: number;
    revoked_refresh_tokens: number;
  }>("/api/client/auth/password/reset/confirm", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export interface MfaSetupResult {
  secret: string;
  otpauth_url: string;
  recovery_codes: string[];
}

export function setupMfa(): Promise<MfaSetupResult> {
  return apiRequest<MfaSetupResult>("/api/auth/mfa/setup", {
    method: "POST",
    body: JSON.stringify({})
  });
}

export function enableMfa(code: string): Promise<{ ok: boolean }> {
  return apiRequest<{ ok: boolean }>("/api/auth/mfa/enable", {
    method: "POST",
    body: JSON.stringify({ code })
  });
}

export function disableMfa(payload: {
  password: string;
  code: string;
}): Promise<{ ok: boolean }> {
  return apiRequest<{ ok: boolean }>("/api/auth/mfa/disable", {
    method: "POST",
    body: JSON.stringify(payload)
  });
}

export function regenerateMfaRecoveryCodes(payload: {
  password: string;
  code: string;
}): Promise<{ recovery_codes: string[] }> {
  return apiRequest<{ recovery_codes: string[] }>(
    "/api/auth/mfa/recovery-codes/regenerate",
    {
      method: "POST",
      body: JSON.stringify(payload)
    }
  );
}
