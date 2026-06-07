const privilegedMfaRoles = new Set(["owner", "admin"]);

export function requiresMfaForRole(roles: string[]): boolean {
  return roles.some((role) => privilegedMfaRoles.has(role));
}
