import type { ServerStatus } from "@/types/instance";

export const STATUS_LABEL: Record<ServerStatus, string> = {
  stopped: "STOPPED",
  starting: "STARTING",
  running: "RUNNING",
  stopping: "STOPPING",
  crashed: "CRASHED",
};

export const STATUS_DOT: Record<ServerStatus, string> = {
  stopped: "bg-muted-foreground",
  starting: "bg-amber-500",
  running: "bg-primary",
  stopping: "bg-amber-500",
  crashed: "bg-destructive",
};

/** Tinted badge classes, one per status - used in place of the plain
 * `variant="outline"` badge so status is legible at a glance, not just via
 * a small dot. */
export const STATUS_BADGE_CLASS: Record<ServerStatus, string> = {
  stopped: "border-transparent bg-muted text-muted-foreground",
  starting: "border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-400",
  running: "border-transparent bg-primary/15 text-primary",
  stopping: "border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-400",
  crashed: "border-transparent bg-destructive/15 text-destructive",
};
