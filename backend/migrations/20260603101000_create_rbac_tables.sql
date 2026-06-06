create table roles (
  id uuid primary key,
  tenant_id uuid not null references tenants(id),
  code text not null,
  name text not null,
  description text null,
  builtin boolean not null default false,
  created_at timestamptz not null default now(),
  updated_at timestamptz not null default now(),
  deleted_at timestamptz null
);

create unique index idx_roles_tenant_code
on roles(tenant_id, code)
where deleted_at is null;

create table permissions (
  id uuid primary key,
  code text not null unique,
  name text not null,
  resource text not null,
  action text not null,
  created_at timestamptz not null default now()
);

create table role_permissions (
  role_id uuid not null references roles(id),
  permission_id uuid not null references permissions(id),
  created_at timestamptz not null default now(),
  primary key (role_id, permission_id)
);

create table team_member_roles (
  team_member_id uuid not null references team_members(id),
  role_id uuid not null references roles(id),
  created_at timestamptz not null default now(),
  primary key (team_member_id, role_id)
);

create extension if not exists pgcrypto;

insert into permissions (id, code, name, resource, action)
values
  (gen_random_uuid(), 'tenant:read', 'Read tenant', 'tenant', 'read'),
  (gen_random_uuid(), 'tenant:update', 'Update tenant', 'tenant', 'update'),
  (gen_random_uuid(), 'tenant:delete', 'Delete tenant', 'tenant', 'delete'),
  (gen_random_uuid(), 'member:read', 'Read team members', 'member', 'read'),
  (gen_random_uuid(), 'member:invite', 'Invite team members', 'member', 'invite'),
  (gen_random_uuid(), 'member:update', 'Update team members', 'member', 'update'),
  (gen_random_uuid(), 'member:disable', 'Disable team members', 'member', 'disable'),
  (gen_random_uuid(), 'member:enable', 'Enable team members', 'member', 'enable'),
  (gen_random_uuid(), 'member:delete', 'Delete team members', 'member', 'delete'),
  (gen_random_uuid(), 'member:reset_password', 'Reset team member password', 'member', 'reset_password'),
  (gen_random_uuid(), 'role:read', 'Read roles', 'role', 'read'),
  (gen_random_uuid(), 'role:create', 'Create roles', 'role', 'create'),
  (gen_random_uuid(), 'role:update', 'Update roles', 'role', 'update'),
  (gen_random_uuid(), 'role:delete', 'Delete roles', 'role', 'delete'),
  (gen_random_uuid(), 'permission:read', 'Read permissions', 'permission', 'read'),
  (gen_random_uuid(), 'customer:read', 'Read customers', 'customer', 'read'),
  (gen_random_uuid(), 'customer:create', 'Create customers', 'customer', 'create'),
  (gen_random_uuid(), 'customer:update', 'Update customers', 'customer', 'update'),
  (gen_random_uuid(), 'customer:disable', 'Disable customers', 'customer', 'disable'),
  (gen_random_uuid(), 'customer:enable', 'Enable customers', 'customer', 'enable'),
  (gen_random_uuid(), 'customer:delete', 'Delete customers', 'customer', 'delete'),
  (gen_random_uuid(), 'customer:reset_password', 'Reset customer password', 'customer', 'reset_password'),
  (gen_random_uuid(), 'app:read', 'Read applications', 'app', 'read'),
  (gen_random_uuid(), 'app:create', 'Create applications', 'app', 'create'),
  (gen_random_uuid(), 'app:update', 'Update applications', 'app', 'update'),
  (gen_random_uuid(), 'app:delete', 'Delete applications', 'app', 'delete'),
  (gen_random_uuid(), 'app:rotate_key', 'Rotate application keys', 'app', 'rotate_key'),
  (gen_random_uuid(), 'app:read_key', 'Read application public keys', 'app', 'read_key'),
  (gen_random_uuid(), 'license:read', 'Read licenses', 'license', 'read'),
  (gen_random_uuid(), 'license:create', 'Create licenses', 'license', 'create'),
  (gen_random_uuid(), 'license:update', 'Update licenses', 'license', 'update'),
  (gen_random_uuid(), 'license:revoke', 'Revoke licenses', 'license', 'revoke'),
  (gen_random_uuid(), 'license:suspend', 'Suspend licenses', 'license', 'suspend'),
  (gen_random_uuid(), 'license:renew', 'Renew licenses', 'license', 'renew'),
  (gen_random_uuid(), 'license:reset_device', 'Reset license devices', 'license', 'reset_device'),
  (gen_random_uuid(), 'subscription:read', 'Read subscriptions', 'subscription', 'read'),
  (gen_random_uuid(), 'subscription:create', 'Create subscriptions', 'subscription', 'create'),
  (gen_random_uuid(), 'subscription:update', 'Update subscriptions', 'subscription', 'update'),
  (gen_random_uuid(), 'subscription:cancel', 'Cancel subscriptions', 'subscription', 'cancel'),
  (gen_random_uuid(), 'subscription:renew', 'Renew subscriptions', 'subscription', 'renew'),
  (gen_random_uuid(), 'device:read', 'Read devices', 'device', 'read'),
  (gen_random_uuid(), 'device:update', 'Update devices', 'device', 'update'),
  (gen_random_uuid(), 'device:unbind', 'Unbind devices', 'device', 'unbind'),
  (gen_random_uuid(), 'device:blacklist', 'Blacklist devices', 'device', 'blacklist'),
  (gen_random_uuid(), 'device:unblacklist', 'Unblacklist devices', 'device', 'unblacklist'),
  (gen_random_uuid(), 'device:revoke_session', 'Revoke device sessions', 'device', 'revoke_session'),
  (gen_random_uuid(), 'release:read', 'Read releases', 'release', 'read'),
  (gen_random_uuid(), 'release:upload', 'Upload release files', 'release', 'upload'),
  (gen_random_uuid(), 'release:create', 'Create releases', 'release', 'create'),
  (gen_random_uuid(), 'release:update', 'Update releases', 'release', 'update'),
  (gen_random_uuid(), 'release:publish', 'Publish releases', 'release', 'publish'),
  (gen_random_uuid(), 'release:deprecate', 'Deprecate releases', 'release', 'deprecate'),
  (gen_random_uuid(), 'release:delete', 'Delete releases', 'release', 'delete'),
  (gen_random_uuid(), 'script:read', 'Read scripts', 'script', 'read'),
  (gen_random_uuid(), 'script:create', 'Create scripts', 'script', 'create'),
  (gen_random_uuid(), 'script:update', 'Update scripts', 'script', 'update'),
  (gen_random_uuid(), 'script:publish', 'Publish scripts', 'script', 'publish'),
  (gen_random_uuid(), 'script:deprecate', 'Deprecate scripts', 'script', 'deprecate'),
  (gen_random_uuid(), 'script:revoke', 'Revoke scripts', 'script', 'revoke'),
  (gen_random_uuid(), 'script:delete', 'Delete scripts', 'script', 'delete'),
  (gen_random_uuid(), 'audit:read', 'Read audit logs', 'audit', 'read'),
  (gen_random_uuid(), 'audit:export', 'Export audit logs', 'audit', 'export'),
  (gen_random_uuid(), 'system:read', 'Read system settings', 'system', 'read'),
  (gen_random_uuid(), 'system:update', 'Update system settings', 'system', 'update'),
  (gen_random_uuid(), 'security:read', 'Read security settings', 'security', 'read'),
  (gen_random_uuid(), 'security:revoke_session', 'Revoke sessions', 'security', 'revoke_session'),
  (gen_random_uuid(), 'security:rotate_key', 'Rotate security keys', 'security', 'rotate_key'),
  (gen_random_uuid(), 'security:view_events', 'View security events', 'security', 'view_events');
