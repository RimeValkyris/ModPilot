import { useEffect } from "react";
import { Outlet } from "react-router-dom";
import { Sidebar } from "@/components/layout/Sidebar";
import { useInstanceStatusEvents } from "@/hooks/useInstanceStatusEvents";
import { useStuckStartingEvents } from "@/hooks/useStuckStartingEvents";
import { useNotificationPermission } from "@/hooks/useNotificationPermission";
import { useResourceUsagePolling } from "@/hooks/useResourceUsagePolling";
import { useThemeStore } from "@/stores/themeStore";

export function AppLayout() {
  useInstanceStatusEvents();
  useStuckStartingEvents();
  useNotificationPermission();
  useResourceUsagePolling();

  const loadTheme = useThemeStore((s) => s.load);
  useEffect(() => {
    loadTheme();
  }, [loadTheme]);

  return (
    <div className="flex h-screen w-screen overflow-hidden bg-background text-foreground">
      <Sidebar />
      <main className="flex-1 overflow-y-auto border-l border-border/60 bg-muted/30">
        <Outlet />
      </main>
    </div>
  );
}
