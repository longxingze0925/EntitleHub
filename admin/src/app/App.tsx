import { Button, Result, Spin } from "antd";
import { useEffect } from "react";
import {
  BrowserRouter,
  Link,
  Navigate,
  Outlet,
  Route,
  Routes,
  useLocation
} from "react-router-dom";
import { useQuery } from "@tanstack/react-query";

import { ApiError } from "../api/client";
import { me } from "../api/auth";
import { AdminLayout } from "../layouts/AdminLayout";
import { DashboardPage } from "../pages/dashboard/DashboardPage";
import { ClientEmailVerifyPage } from "../pages/login/ClientEmailVerifyPage";
import { ClientPasswordResetPage } from "../pages/login/ClientPasswordResetPage";
import { EmailVerifyPage } from "../pages/login/EmailVerifyPage";
import { LoginPage } from "../pages/login/LoginPage";
import { PasswordResetPage } from "../pages/login/PasswordResetPage";
import { ApplicationsPage } from "../pages/resources/ApplicationsPage";
import { AuditLogsPage } from "../pages/resources/AuditLogsPage";
import { CustomersPage } from "../pages/resources/CustomersPage";
import { DevicesPage } from "../pages/resources/DevicesPage";
import { LicensesPage } from "../pages/resources/LicensesPage";
import { NotificationChannelsPage } from "../pages/resources/NotificationChannelsPage";
import { OutboxEventsPage } from "../pages/resources/OutboxEventsPage";
import { ReleasesPage } from "../pages/resources/ReleasesPage";
import { RolesPage } from "../pages/resources/RolesPage";
import { ScriptsPage } from "../pages/resources/ScriptsPage";
import { SubscriptionsPage } from "../pages/resources/SubscriptionsPage";
import { SystemSettingsPage } from "../pages/resources/SystemSettingsPage";
import { SecurityPage } from "../pages/security/SecurityPage";
import { TeamPage } from "../pages/resources/TeamPage";
import { menuRoutes } from "../routes/menu";
import { useAuthStore } from "../stores/authStore";
import { hasPermission } from "../utils/permissions";

function ProtectedRoutes() {
  const location = useLocation();
  const { user, permissions, clear, setProfile } = useAuthStore();
  const profileQuery = useQuery({
    queryKey: ["auth", "me"],
    queryFn: me,
    enabled: !user
  });

  useEffect(() => {
    if (profileQuery.data) {
      setProfile(profileQuery.data);
    }
  }, [profileQuery.data, setProfile]);

  useEffect(() => {
    if (
      profileQuery.error instanceof ApiError &&
      profileQuery.error.status === 401
    ) {
      clear();
    }
  }, [clear, profileQuery.error]);

  if (!user && profileQuery.isPending) {
    return (
      <div className="app-loading">
        <Spin />
      </div>
    );
  }

  if (!user && profileQuery.error) {
    return (
      <Navigate to="/login" replace state={{ from: location.pathname }} />
    );
  }

  const currentRoute = menuRoutes.find(
    (route) => route.path === location.pathname
  );
  if (currentRoute && !hasPermission(permissions, currentRoute.permission)) {
    return (
      <AdminLayout>
        <Result
          status="403"
          title="403"
          subTitle="无权限访问"
          extra={
            <Button type="primary">
              <Link to="/">返回仪表盘</Link>
            </Button>
          }
        />
      </AdminLayout>
    );
  }

  return (
    <AdminLayout>
      <Outlet />
    </AdminLayout>
  );
}

export function App() {
  return (
    <BrowserRouter>
      <Routes>
        <Route path="/login" element={<LoginPage />} />
        <Route path="/admin/password-reset" element={<PasswordResetPage />} />
        <Route path="/admin/email-verify" element={<EmailVerifyPage />} />
        <Route path="/client/password-reset" element={<ClientPasswordResetPage />} />
        <Route path="/client/email-verify" element={<ClientEmailVerifyPage />} />
        <Route element={<ProtectedRoutes />}>
          <Route index element={<DashboardPage />} />
          <Route path="team" element={<TeamPage />} />
          <Route path="roles" element={<RolesPage />} />
          <Route path="customers" element={<CustomersPage />} />
          <Route path="apps" element={<ApplicationsPage />} />
          <Route path="licenses" element={<LicensesPage />} />
          <Route path="subscriptions" element={<SubscriptionsPage />} />
          <Route path="devices" element={<DevicesPage />} />
          <Route path="releases" element={<ReleasesPage />} />
          <Route path="scripts" element={<ScriptsPage />} />
          <Route path="audit" element={<AuditLogsPage />} />
          <Route path="system-settings" element={<SystemSettingsPage />} />
          <Route path="notification-channels" element={<NotificationChannelsPage />} />
          <Route path="outbox" element={<OutboxEventsPage />} />
          <Route path="security" element={<SecurityPage />} />
        </Route>
        <Route path="*" element={<Navigate to="/" replace />} />
      </Routes>
    </BrowserRouter>
  );
}
