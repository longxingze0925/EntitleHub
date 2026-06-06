export interface ApiEnvelope<T> {
  code: number;
  message: string;
  data: T;
  request_id: string;
}

export interface ApiErrorBody {
  code: number;
  message: string;
  data: null;
  request_id: string;
}

export interface AdminUser {
  id: string;
  email: string;
  name: string;
  email_verified?: boolean;
  mfa_enabled?: boolean;
}

export interface AdminTenant {
  id: string;
  name: string;
}

export interface AuthProfile {
  user: AdminUser;
  tenant: AdminTenant;
  roles: string[];
  permissions: string[];
}

export interface LoginResponse {
  user: AdminUser;
  tenant: AdminTenant;
  roles: string[];
  permissions: string[];
}
