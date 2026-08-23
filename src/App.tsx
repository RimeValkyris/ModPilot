import { lazy, Suspense } from "react";
import { HashRouter, Routes, Route, Navigate } from "react-router-dom";
import { AppLayout } from "@/components/layout/AppLayout";
import { Dashboard } from "@/features/dashboard/Dashboard";
import { ServersPage } from "@/features/servers/ServersPage";
import { Toaster } from "@/components/ui/sonner";
import { ErrorBoundary } from "@/components/ErrorBoundary";
import { CloseGuard } from "@/components/CloseGuard";

// Lazy-loaded: each of these pulls in a fair amount that isn't needed just
// to show the dashboard on launch (the instance detail page alone drags in
// the console, mods/files/players tabs, and the settings form). Splitting
// them out keeps the initial bundle - and so first paint - smaller.
const JavaPage = lazy(() =>
  import("@/features/java/JavaPage").then((m) => ({ default: m.JavaPage })),
);
const SettingsPage = lazy(() =>
  import("@/features/settings/SettingsPage").then((m) => ({ default: m.SettingsPage })),
);
const InstanceDetailPage = lazy(() =>
  import("@/features/console/InstanceDetailPage").then((m) => ({
    default: m.InstanceDetailPage,
  })),
);

function PageFallback() {
  return <div className="p-8 text-sm text-muted-foreground">Loading…</div>;
}

// HashRouter (not BrowserRouter): ModpackPilot is served from the filesystem via
// Tauri's webview, not from a real HTTP server, so there's no server to
// handle history-API deep links.
function App() {
  return (
    <ErrorBoundary>
      <HashRouter>
        <Suspense fallback={<PageFallback />}>
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
        </Suspense>
        <Toaster />
        <CloseGuard />
      </HashRouter>
    </ErrorBoundary>
  );
}

export default App;
