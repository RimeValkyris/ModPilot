import { cn } from "@/lib/utils";

/**
 * Default server icon shown before a custom profile picture is set - a 2x2
 * checkered grid, matching the "unknown server" placeholder Minecraft's own
 * multiplayer list shows for servers without a `server-icon.png`.
 */
export function ServerAvatarPlaceholder({ className }: { className?: string }) {
  return (
    <div className={cn("grid grid-cols-2 grid-rows-2 overflow-hidden", className)}>
      <div className="bg-muted" />
      <div className="bg-muted-foreground/20" />
      <div className="bg-muted-foreground/20" />
      <div className="bg-muted" />
    </div>
  );
}
