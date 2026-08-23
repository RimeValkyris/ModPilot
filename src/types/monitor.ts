/** Mirrors Rust's `ResourceUsage`. */
export interface ResourceUsage {
  isRunning: boolean;
  cpuPercent: number;
  memoryMb: number;
  uptimeSeconds: number;
}
