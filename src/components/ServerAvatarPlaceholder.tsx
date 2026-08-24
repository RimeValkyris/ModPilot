import defaultAvatar from "@/assets/default-server-avatar.jpg";
import { cn } from "@/lib/utils";

/**
 * Default server icon shown before a custom profile picture is set.
 * Bundled as a real asset (not a data URI) so it's cached like any other
 * image instead of being re-embedded into every page load.
 */
export function ServerAvatarPlaceholder({ className }: { className?: string }) {
  return (
    <img src={defaultAvatar} alt="" className={cn("object-cover", className)} />
  );
}
