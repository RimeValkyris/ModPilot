import { useEffect } from "react";
import { Outlet } from "react-router-dom";
import { Sidebar } from "@/components/layout/Sidebar";
import { useInstanceStatusEvents } from "@/hooks/useInstanceStatusEvents";
import { useNotificationPermission } from "@/hooks/useNotificationPermission";
import { useResourceUsagePolling } from "@/hooks/useResourceUsagePolling";
import { useThemeStore } from "@/stores/themeStore";
import { useAppWallpaperStore } from "@/stores/appWallpaperStore";

export function AppLayout() {
  useInstanceStatusEvents();
  useNotificationPermission();
  useResourceUsagePolling();

  const loadTheme = useThemeStore((s) => s.load);
  useEffect(() => {
    loadTheme();
  }, [loadTheme]);

  const { wallpaper, blur, dim, load: loadWallpaper } = useAppWallpaperStore();
  useEffect(() => {
    loadWallpaper();
  }, [loadWallpaper]);

  return (
    <div className="relative h-screen w-screen overflow-hidden bg-background text-foreground">
      {/* Fixed, full-viewport background layer - deliberately outside the
          sidebar/main flex flow so it always covers the whole window,
          rather than only the gaps between opaque panels. */}
      {wallpaper && (
        <>
          <div
            className="pointer-events-none fixed inset-0 z-0"
            style={{
              backgroundImage: `url(${wallpaper})`,
              backgroundSize: "cover",
              backgroundPosition: "center",
              filter: blur > 0 ? `blur(${blur}px)` : undefined,
              // Scale up slightly so a blur radius never reveals an edge.
              transform: blur > 0 ? "scale(1.05)" : undefined,
            }}
          />
          <div
            className="pointer-events-none fixed inset-0 z-0"
            style={{ backgroundColor: `rgba(0,0,0,${dim / 100})` }}
          />
        </>
      )}

      <div className="relative z-10 flex h-full w-full">
        <Sidebar />
        <main className="flex-1 overflow-y-auto">
          <Outlet />
        </main>
      </div>
    </div>
  );
}
