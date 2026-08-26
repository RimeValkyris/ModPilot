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

/** Mirrors Rust's `PlayersChangedPayload`. */
export interface PlayersChangedPayload {
  instanceId: string;
  players: string[];
}

/** Mirrors Rust's `ResourceAlertPayload`. */
export interface ResourceAlertPayload {
  instanceId: string;
  kind: string;
  message: string;
}

/** Mirrors Rust's `StuckStartingPayload`, emitted as `instance-stuck-starting`. */
export interface StuckStartingPayload {
  instanceId: string;
}

export const STATUS_EVENT = "instance-status-changed";
export const LOG_EVENT = "instance-log";
export const STUCK_STARTING_EVENT = "instance-stuck-starting";
export const PLAYERS_EVENT = "instance-players-changed";
export const RESOURCE_ALERT_EVENT = "instance-resource-alert";
