export function hasPermission(permissions: string[], required?: string): boolean {
  if (!required) {
    return true;
  }

  return permissions.includes(required);
}

export function hasAnyPermission(
  permissions: string[],
  required: string[]
): boolean {
  return required.some((permission) => permissions.includes(permission));
}
