import { useResourceUsageStore } from "@/stores/resourceUsageStore";
import { formatMemoryMb, formatUptime } from "@/lib/format";
import type { Instance } from "@/types/instance";

/**
 * Compact CPU/RAM/disk/uptime readout for an instance card, shown only
 * while it is RUNNING - there's nothing to measure otherwise.
 *
 * Deliberately the short form. The full six-metric readout with charts
 * lives on the instance's own Overview tab (`InstanceDashboard`); a card in
 * a list needs the numbers that fit at a glance, and a metric that can't be
 * measured right now is omitted rather than shown as a zero.
 *
 * Reads from the single app-wide resource poll (`useResourceUsagePolling`,
 * mounted once at the app root) rather than polling itself - with several
 * of these mounted at once, independent per-card polling would mean one
 * backend round trip per card per tick for data one shared call covers.
 */
export function ResourceUsageRow({ instance }: { instance: Instance }) {
  const usage = useResourceUsageStore((s) => s.usageByInstanceId[instance.id]);

  if (!usage || !usage.isRunning) {
    return (
      <div className="text-xs text-muted-foreground">Waiting for resource data…</div>
    );
  }

  return (
    <div className="grid grid-cols-2 gap-2 text-xs text-muted-foreground sm:grid-cols-4">
      <div>
        <span className="text-foreground">{usage.cpuPercent.toFixed(0)}%</span> CPU
      </div>
      <div>
        <span className="text-foreground">{formatMemoryMb(usage.memoryMb)}</span> /{" "}
        {formatMemoryMb(instance.maxRamMb)}
      </div>
      <div title={usage.disk ? `Used on ${usage.disk.mountPoint}` : undefined}>
        {usage.disk ? (
          <>
            <span className="text-foreground">{usage.disk.usedPercent.toFixed(0)}%</span> disk
          </>
        ) : (
          <span className="text-muted-foreground">— disk</span>
        )}
      </div>
      <div>
        <span className="text-foreground">{formatUptime(usage.uptimeSeconds)}</span>
      </div>
    </div>
  );
}
