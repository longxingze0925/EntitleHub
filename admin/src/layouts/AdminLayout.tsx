import { Button, Layout, Menu, Space, Typography } from "antd";
import { LogOut, Menu as MenuIcon } from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { logout } from "../api/auth";
import { menuRoutes } from "../routes/menu";
import { useAuthStore } from "../stores/authStore";
import { hasPermission } from "../utils/permissions";

const { Header, Sider, Content } = Layout;

interface AdminLayoutProps {
  children: React.ReactNode;
}

export function AdminLayout({ children }: AdminLayoutProps) {
  const [collapsed, setCollapsed] = useState(false);
  const location = useLocation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const { user, tenant, permissions, clear } = useAuthStore();

  const visibleRoutes = useMemo(
    () =>
      menuRoutes.filter((route) =>
        hasPermission(permissions, route.permission)
      ),
    [permissions]
  );
  const selectedKey =
    visibleRoutes.find((route) => route.path === location.pathname)?.key ??
    "dashboard";

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
          <span>UA</span>
          {!collapsed ? <strong>用户管理后台</strong> : null}
        </div>
        <Menu
          mode="inline"
          selectedKeys={[selectedKey]}
          items={visibleRoutes.map((route) => ({
            key: route.key,
            icon: route.icon,
            label: <Link to={route.path}>{route.label}</Link>
          }))}
        />
      </Sider>

      <Layout>
        <Header className="admin-header">
          <Button
            icon={<MenuIcon size={17} />}
            onClick={() => setCollapsed((value) => !value)}
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
