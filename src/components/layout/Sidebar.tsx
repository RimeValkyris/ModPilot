import { NavLink } from "react-router-dom";
import { LayoutDashboard, Server, Coffee, Settings } from "lucide-react";
import { cn } from "@/lib/utils";
import { useAppWallpaperStore } from "@/stores/appWallpaperStore";

const navItems = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, end: true },
  { to: "/servers", label: "Servers", icon: Server },
  { to: "/java", label: "Java", icon: Coffee },
  { to: "/settings", label: "Settings", icon: Settings },
];

export function Sidebar() {
  const hasWallpaper = useAppWallpaperStore((s) => s.wallpaper !== null);

  return (
    <aside
      className={cn(
        "flex h-full w-56 shrink-0 flex-col border-r border-border",
        hasWallpaper ? "bg-card/70 backdrop-blur-md" : "bg-card",
      )}
    >
      <div className="flex h-14 items-center gap-2 border-b border-border px-4">
        <span className="text-lg font-semibold tracking-tight">ModForge</span>
      </div>
      <nav className="flex flex-col gap-1 p-2">
        {navItems.map(({ to, label, icon: Icon, end }) => (
          <NavLink
            key={to}
            to={to}
            end={end}
            className={({ isActive }) =>
              cn(
                "flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm font-medium transition-colors",
                isActive
                  ? "bg-secondary text-secondary-foreground"
                  : "text-muted-foreground hover:bg-muted hover:text-foreground",
              )
            }
          >
            <Icon className="size-4" />
            {label}
          </NavLink>
        ))}
      </nav>
    </aside>
  );
}
