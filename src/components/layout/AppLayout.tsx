import { useEffect } from "react";
import { Outlet } from "react-router-dom";
import { Sidebar } from "@/components/layout/Sidebar";
import { useInstanceStatusEvents } from "@/hooks/useInstanceStatusEvents";
import { useNotificationPermission } from "@/hooks/useNotificationPermission";
import { useResourceUsagePolling } from "@/hooks/useResourceUsagePolling";
import { useThemeStore } from "@/stores/themeStore";

export function AppLayout() {
  useInstanceStatusEvents();
  useNotificationPermission();
  useResourceUsagePolling();

  const loadTheme = useThemeStore((s) => s.load);
  useEffect(() => {
    loadTheme();
  }, [loadTheme]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <Sidebar />
      <main className="flex-1 overflow-y-auto">
        <Outlet />
      </main>
    </div>
  );
}
