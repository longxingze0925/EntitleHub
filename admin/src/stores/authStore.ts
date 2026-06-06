import { create } from "zustand";

import type { AdminTenant, AdminUser } from "../types/api";

interface AuthState {
  user: AdminUser | null;
  tenant: AdminTenant | null;
  roles: string[];
  permissions: string[];
  setProfile: (profile: {
    user: AdminUser;
    tenant: AdminTenant;
    roles?: string[];
    permissions: string[];
  }) => void;
  clear: () => void;
}

export const useAuthStore = create<AuthState>((set) => ({
  user: null,
  tenant: null,
  roles: [],
  permissions: [],
  setProfile: (profile) =>
    set({
      user: profile.user,
      tenant: profile.tenant,
      roles: profile.roles ?? [],
      permissions: profile.permissions
    }),
  clear: () =>
    set({
      user: null,
      tenant: null,
      roles: [],
      permissions: []
    })
}));
