import { useEffect, useState } from "react";
import { NavLink } from "react-router-dom";
import { getVersion } from "@tauri-apps/api/app";
import { LayoutDashboard, Server, Coffee, Settings } from "lucide-react";
import { cn } from "@/lib/utils";
import umaCube from "@/assets/uma-cube.png";

const navItems = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, end: true },
  { to: "/servers", label: "Servers", icon: Server },
  { to: "/java", label: "Java", icon: Coffee },
  { to: "/settings", label: "Settings", icon: Settings },
];

/** The running build's version, read from the app itself rather than
 *  hard-coded - a literal here went stale the first time the app shipped. */
function useAppVersion() {
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);
  return version;
}

export function Sidebar() {
  const version = useAppVersion();

  return (
    <aside
      data-slot="app-sidebar"
      className="flex h-full w-60 shrink-0 flex-col bg-sidebar text-sidebar-foreground"
    >
      <div className="flex h-16 items-center gap-2.5 px-5">
        <img
          data-slot="app-logo"
          src={umaCube}
          alt=""
          className="size-8 shrink-0 rounded-lg object-cover shadow-sm"
        />
        <span className="text-[15px] font-semibold tracking-tight">ModpackPilot</span>
      </div>

      <nav className="flex flex-col gap-1 px-3 pt-2">
        <p className="px-2.5 pb-1.5 text-[11px] font-medium tracking-wider text-muted-foreground/70 uppercase">
          Menu
        </p>
        {navItems.map(({ to, label, icon: Icon, end }) => (
          <NavLink
            key={to}
            to={to}
            end={end}
            data-slot="nav-item"
            className={({ isActive }) =>
              cn(
                "group relative flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm font-medium transition-[color,background-color,box-shadow]",
                isActive
                  ? "bg-sidebar-accent text-sidebar-accent-foreground"
                  : "text-muted-foreground hover:bg-sidebar-accent/50 hover:text-foreground",
              )
            }
          >
            {({ isActive }) => (
              <>
                <span
                  data-slot="nav-indicator"
                  className={cn(
                    "absolute left-0 h-4.5 w-0.75 rounded-full bg-primary transition-opacity",
                    isActive ? "opacity-100" : "opacity-0",
                  )}
                />
                <Icon
                  className={cn(
                    "size-4 transition-colors",
                    isActive ? "text-primary" : "text-muted-foreground group-hover:text-foreground",
                  )}
                />
                {label}
              </>
            )}
          </NavLink>
        ))}
      </nav>

      {version && (
        <div className="mt-auto flex items-center gap-2 px-5 py-4 text-xs text-muted-foreground/70">
          <span className="size-1.5 rounded-full bg-primary" />v{version}
        </div>
      )}
    </aside>
  );
}
