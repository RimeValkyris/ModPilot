import { NavLink } from "react-router-dom";
import { LayoutDashboard, Server, Coffee, Settings, Zap } from "lucide-react";
import { cn } from "@/lib/utils";

const navItems = [
  { to: "/", label: "Dashboard", icon: LayoutDashboard, end: true },
  { to: "/servers", label: "Servers", icon: Server },
  { to: "/java", label: "Java", icon: Coffee },
  { to: "/settings", label: "Settings", icon: Settings },
];

export function Sidebar() {
  return (
    <aside className="flex h-full w-60 shrink-0 flex-col bg-sidebar text-sidebar-foreground">
      <div className="flex h-16 items-center gap-2.5 px-5">
        <div className="flex size-8 items-center justify-center rounded-lg bg-primary text-primary-foreground shadow-sm shadow-primary/30">
          <Zap className="size-4.5 fill-current" strokeWidth={0} />
        </div>
        <span className="text-[15px] font-semibold tracking-tight">ModpackPilot</span>
      </div>

      <nav className="flex flex-col gap-0.5 px-3 pt-2">
        <p className="px-2.5 pb-1.5 text-[11px] font-medium tracking-wider text-muted-foreground/70 uppercase">
          Menu
        </p>
        {navItems.map(({ to, label, icon: Icon, end }) => (
          <NavLink
            key={to}
            to={to}
            end={end}
            className={({ isActive }) =>
              cn(
                "group relative flex items-center gap-2.5 rounded-lg px-2.5 py-2 text-sm font-medium transition-colors",
                isActive
                  ? "bg-sidebar-accent text-sidebar-accent-foreground"
                  : "text-muted-foreground hover:bg-sidebar-accent/50 hover:text-foreground",
              )
            }
          >
            {({ isActive }) => (
              <>
                <span
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

      <div className="mt-auto flex items-center gap-2 px-5 py-4 text-xs text-muted-foreground/70">
        <span className="size-1.5 rounded-full bg-primary" />
        v0.1.0
      </div>
    </aside>
  );
}
