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
  /** Milliseconds per tick. More diagnostic than TPS, which saturates at 20
   * and so reads the same for a comfortable server and one about to fall
   * over. Null when the server reported a rate without a time. */
  mspt: number | null;
  /** Size of the console-derived roster, used when the ping didn't report a
   * player count. */
  playersTracked: number;
  health: ServerHealth;
}

/** Mirrors Rust's `HealthStatus`. `unknown` means the server isn't running,
 * not that something went wrong. */
export type HealthStatus = "critical" | "warning" | "healthy" | "unknown";

/** Mirrors Rust's `ServerHealth`. `reasons` is empty when nothing is wrong -
 * a verdict is never shown without what produced it. */
export interface ServerHealth {
  status: HealthStatus;
  reasons: string[];
}

/** Mirrors Rust's `PerformanceSample` - one row of recorded history.
 *
 * Nulls mean "not measurable at that moment", so a chart draws a gap rather
 * than a dip to zero. */
export interface PerformanceSample {
  recordedAt: string;
  cpuPercent: number;
  memoryMb: number;
  tps: number | null;
  mspt: number | null;
  players: number | null;
  pingMs: number | null;
}
