import type { ServerStatus } from "@/types/instance";

/** Mirrors Rust's `StatusChangedPayload`, emitted as `instance-status-changed`. */
export interface StatusChangedPayload {
  instanceId: string;
  status: ServerStatus;
}

/** Mirrors Rust's `LogLinePayload`, emitted as `instance-log`. */
export interface LogLinePayload {
  instanceId: string;
  stream: "stdout" | "stderr";
  line: string;
}

export const STATUS_EVENT = "instance-status-changed";
export const LOG_EVENT = "instance-log";
