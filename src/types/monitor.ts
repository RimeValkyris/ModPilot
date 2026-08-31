/** Mirrors Rust's `DiskUsage`. The volume the instance lives on, not the
 * size of the instance folder. */
export interface DiskUsage {
  usedPercent: number;
  usedBytes: number;
  totalBytes: number;
  mountPoint: string;
}

/** Mirrors Rust's `ServerPing`. */
export interface ServerPing {
  latencyMs: number;
  playersOnline: number | null;
  playersMax: number | null;
}

/**
 * Mirrors Rust's `ResourceUsage`.
 *
 * The nullable fields are nullable on purpose: a metric that can't be read
 * right now is absent rather than zero, so the UI can say "unavailable"
 * instead of drawing a number that looks measured. `ping` is null while a
 * server is still booting; `tps` is null when the loader has no tick-rate
 * command at all.
 */
export interface ResourceUsage {
  isRunning: boolean;
  cpuPercent: number;
  memoryMb: number;
  uptimeSeconds: number;
  disk: DiskUsage | null;
  ping: ServerPing | null;
  tps: number | null;
  /** Size of the console-derived roster, used when the ping didn't report a
   * player count. */
  playersTracked: number;
}
