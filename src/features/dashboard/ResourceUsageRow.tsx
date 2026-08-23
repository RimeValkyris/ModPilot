import { useResourceUsage } from "@/hooks/useResourceUsage";
import { formatMemoryMb, formatUptime } from "@/lib/format";
import type { Instance } from "@/types/instance";

/**
 * Compact CPU/RAM/uptime readout, shown only while an instance is RUNNING -
 * there's nothing to measure otherwise. No TPS or player count here: those
 * can't be read from the OS process, and ModForge doesn't parse the
 * server's own stats reliably enough yet to display them without risking a
 * misleading number.
 */
export function ResourceUsageRow({ instance }: { instance: Instance }) {
  const usage = useResourceUsage(instance.id, instance.status === "running");

  if (!usage || !usage.isRunning) {
    return (
      <div className="text-xs text-muted-foreground">Waiting for resource data…</div>
    );
  }

  return (
    <div className="grid grid-cols-3 gap-2 text-xs text-muted-foreground">
      <div>
        <span className="text-foreground">{usage.cpuPercent.toFixed(0)}%</span> CPU
      </div>
      <div>
        <span className="text-foreground">{formatMemoryMb(usage.memoryMb)}</span> /{" "}
        {formatMemoryMb(instance.maxRamMb)}
      </div>
      <div>
        <span className="text-foreground">{formatUptime(usage.uptimeSeconds)}</span>
      </div>
    </div>
  );
}
