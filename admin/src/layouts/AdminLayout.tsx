import { Button, Drawer, Layout, Menu, Space, Typography } from "antd";
import type { MenuProps } from "antd";
import { LogOut, Menu as MenuIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { logout } from "../api/auth";
import { flatMenuRoutes, menuRoutes, type MenuRoute } from "../routes/menu";
import { useAuthStore } from "../stores/authStore";
import { hasPermission } from "../utils/permissions";

const { Header, Sider, Content } = Layout;

interface AdminLayoutProps {
  children: React.ReactNode;
}

export function AdminLayout({ children }: AdminLayoutProps) {
  const [collapsed, setCollapsed] = useState(false);
  const [isMobile, setIsMobile] = useState(false);
  const [mobileMenuOpen, setMobileMenuOpen] = useState(false);
  const [openMenuKeys, setOpenMenuKeys] = useState<string[]>([]);
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { user, tenant, permissions, clear } = useAuthStore();

  const visibleRoutes = useMemo(
    () => filterVisibleRoutes(menuRoutes, permissions),
    [permissions]
  );
  const visibleFlatRoutes = useMemo(
    () =>
      flatMenuRoutes.filter((route) =>
        hasPermission(permissions, route.permission)
      ),
    [permissions]
  );
  const selectedKey =
    visibleFlatRoutes.find((route) => route.path === location.pathname)?.key ??
    "dashboard";
  const activeOpenKeys = useMemo(
    () => parentKeysForPath(visibleRoutes, location.pathname),
    [location.pathname, visibleRoutes]
  );
  const menuItems = useMemo(
    () => visibleRoutes.map(toMenuItem),
    [visibleRoutes]
  );

  useEffect(() => {
    const mediaQuery = window.matchMedia("(max-width: 720px)");
    const handleChange = () => setIsMobile(mediaQuery.matches);
    handleChange();
    mediaQuery.addEventListener("change", handleChange);

    return () => mediaQuery.removeEventListener("change", handleChange);
  }, []);

  useEffect(() => {
    setMobileMenuOpen(false);
  }, [location.pathname]);

  useEffect(() => {
    if (activeOpenKeys.length === 0) {
      return;
    }

    setOpenMenuKeys((current) =>
      Array.from(new Set([...current, ...activeOpenKeys]))
    );
  }, [activeOpenKeys]);

  const logoutMutation = useMutation({
    mutationFn: logout,
    onSettled: () => {
      clear();
      queryClient.clear();
      navigate("/login", { replace: true });
    }
  });

  return (
    <Layout className="admin-shell">
      <Sider
        width={244}
        collapsedWidth={72}
        collapsed={collapsed}
        theme="light"
        className="admin-sider"
      >
        <div className="brand-mark">
          <span>EH</span>
          {!collapsed ? <strong>EntitleHub</strong> : null}
        </div>
        <Menu
          mode="inline"
          selectedKeys={[selectedKey]}
          openKeys={openMenuKeys}
          onOpenChange={(keys) => setOpenMenuKeys(keys)}
          items={menuItems}
        />
      </Sider>
      <Drawer
        className="admin-menu-drawer"
        placement="left"
        width={244}
        open={mobileMenuOpen}
        onClose={() => setMobileMenuOpen(false)}
        closable={false}
        styles={{ body: { padding: 0 } }}
      >
        <div className="brand-mark">
          <span>EH</span>
          <strong>EntitleHub</strong>
        </div>
        <Menu
          mode="inline"
          selectedKeys={[selectedKey]}
          openKeys={openMenuKeys}
          onOpenChange={(keys) => setOpenMenuKeys(keys)}
          items={menuItems}
        />
      </Drawer>

      <Layout>
        <Header className="admin-header">
          <Button
            icon={<MenuIcon size={17} />}
            onClick={() => {
              if (isMobile) {
                setMobileMenuOpen(true);
                return;
              }

              setCollapsed((value) => !value);
            }}
          />
          <Space className="header-account" size={14}>
            <div className="header-identity">
              <Typography.Text strong>{user?.name ?? "-"}</Typography.Text>
              <Typography.Text type="secondary">
                {tenant?.name ?? "-"}
              </Typography.Text>
            </div>
            <Button
              icon={<LogOut size={16} />}
              onClick={() => logoutMutation.mutate()}
              loading={logoutMutation.isPending}
            />
          </Space>
        </Header>

        <Content className="admin-content">{children}</Content>
      </Layout>
    </Layout>
  );
}

function filterVisibleRoutes(routes: MenuRoute[], permissions: string[]): MenuRoute[] {
  const visibleRoutes: MenuRoute[] = [];

  for (const route of routes) {
    const children = route.children
      ? filterVisibleRoutes(route.children, permissions)
      : undefined;
    const visible = hasPermission(permissions, route.permission);

    if (visible || children?.length) {
      visibleRoutes.push({
        ...route,
        children
      });
    }
  }

  return visibleRoutes;
}

function toMenuItem(route: MenuRoute): NonNullable<MenuProps["items"]>[number] {
  return {
    key: route.key,
    icon: route.icon,
    label: route.children?.length ? (
      route.label
    ) : (
      <Link to={route.path}>{route.label}</Link>
    ),
    children: route.children?.map(toMenuItem)
  };
}

function parentKeysForPath(routes: MenuRoute[], path: string): string[] {
  for (const route of routes) {
    if (route.children?.some((child) => child.path === path)) {
      return [route.key];
    }

    if (route.children?.length) {
      const nested = parentKeysForPath(route.children, path);
      if (nested.length > 0) {
        return [route.key, ...nested];
      }
    }
  }

  return [];
}
