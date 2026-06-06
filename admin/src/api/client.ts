import type { ApiEnvelope, ApiErrorBody } from "../types/api";
import { useAuthStore } from "../stores/authStore";
import type { LoginResponse } from "../types/api";

export class ApiError extends Error {
  code: number;
  requestId: string;
  status: number;

  constructor(status: number, body: ApiErrorBody) {
    super(body.message);
    this.name = "ApiError";
    this.status = status;
    this.code = body.code;
    this.requestId = body.request_id;
  }
}

const REFRESHABLE_CODES = new Set([40100, 40101]);
const CSRF_COOKIE = "admin_csrf";
const CSRF_HEADER = "X-CSRF-Token";
let refreshPromise: Promise<boolean> | null = null;

export async function apiRequest<T>(
  path: string,
  init: RequestInit = {}
): Promise<T> {
  try {
    return await performRequest<T>(path, init);
  } catch (error) {
    if (!(error instanceof ApiError) || !shouldRefresh(path, error)) {
      throw error;
    }

    const refreshed = await refreshSession();
    if (!refreshed) {
      throw error;
    }

    return performRequest<T>(path, init);
  }
}

async function performRequest<T>(
  path: string,
  init: RequestInit = {}
): Promise<T> {
  const response = await fetch(path, {
    ...init,
    credentials: "include",
    headers: buildHeaders(init)
  });

  const body = (await response.json()) as ApiEnvelope<T> | ApiErrorBody;
  if (!response.ok || body.code !== 0) {
    throw new ApiError(response.status, body as ApiErrorBody);
  }

  return (body as ApiEnvelope<T>).data;
}

function shouldRefresh(path: string, error: ApiError): boolean {
  if (path === "/api/auth/login" || path === "/api/auth/refresh") {
    return false;
  }

  return error.status === 401 && REFRESHABLE_CODES.has(error.code);
}

async function refreshSession(): Promise<boolean> {
  if (!refreshPromise) {
    refreshPromise = requestRefresh()
      .then((profile) => {
        useAuthStore.getState().setProfile(profile);

        return true;
      })
      .catch(() => {
        useAuthStore.getState().clear();

        return false;
      })
      .finally(() => {
        refreshPromise = null;
      });
  }

  return refreshPromise;
}

async function requestRefresh(): Promise<LoginResponse> {
  const response = await fetch("/api/auth/refresh", {
    method: "POST",
    credentials: "include",
    headers: buildHeaders({ method: "POST" })
  });
  const body = (await response.json()) as ApiEnvelope<LoginResponse> | ApiErrorBody;

  if (!response.ok || body.code !== 0) {
    throw new ApiError(response.status, body as ApiErrorBody);
  }

  return (body as ApiEnvelope<LoginResponse>).data;
}

function buildHeaders(init: RequestInit): HeadersInit {
  const headers: Record<string, string> = {
    "Content-Type": "application/json",
    ...normalizeHeaders(init.headers)
  };
  const method = (init.method ?? "GET").toUpperCase();
  const csrfToken = readCookie(CSRF_COOKIE);

  if (csrfToken && !["GET", "HEAD", "OPTIONS"].includes(method)) {
    headers[CSRF_HEADER] = csrfToken;
  }

  return headers;
}

function normalizeHeaders(headers?: HeadersInit): Record<string, string> {
  if (!headers) {
    return {};
  }

  if (headers instanceof Headers) {
    return Object.fromEntries(headers.entries());
  }

  if (Array.isArray(headers)) {
    return Object.fromEntries(headers);
  }

  return headers;
}

function readCookie(name: string): string | null {
  const prefix = `${name}=`;
  const cookie = document.cookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith(prefix));

  return cookie ? decodeURIComponent(cookie.slice(prefix.length)) : null;
}
