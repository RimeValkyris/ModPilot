import { HashRouter, Routes, Route, Navigate } from "react-router-dom";
import { AppLayout } from "@/components/layout/AppLayout";
import { Dashboard } from "@/features/dashboard/Dashboard";
import { ServersPage } from "@/features/servers/ServersPage";
import { JavaPage } from "@/features/java/JavaPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { InstanceDetailPage } from "@/features/console/InstanceDetailPage";
import { Toaster } from "@/components/ui/sonner";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { CloseGuard } from "@/components/CloseGuard";

// HashRouter (not BrowserRouter): ModForge is served from the filesystem via
// Tauri's webview, not from a real HTTP server, so there's no server to
// handle history-API deep links.
function App() {
  return (
    <ErrorBoundary>
      <HashRouter>
        <Routes>
          <Route element={<AppLayout />}>
            <Route index element={<Dashboard />} />
            <Route path="servers" element={<ServersPage />} />
            <Route path="instances/:id" element={<InstanceDetailPage />} />
            <Route path="instances/:id/:tab" element={<InstanceDetailPage />} />
            <Route path="java" element={<JavaPage />} />
            <Route path="settings" element={<SettingsPage />} />
            <Route path="*" element={<Navigate to="/" replace />} />
          </Route>
        </Routes>
        <Toaster />
        <CloseGuard />
      </HashRouter>
    </ErrorBoundary>
  );
}

export default App;
