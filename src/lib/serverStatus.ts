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
  starting: "bg-yellow-500",
  running: "bg-green-500",
  stopping: "bg-yellow-500",
  crashed: "bg-destructive",
};
