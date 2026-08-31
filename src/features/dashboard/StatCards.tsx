import { useResourceUsageStore } from "@/stores/resourceUsageStore";
import { formatMemoryMb } from "@/lib/format";
import type { Instance } from "@/types/instance";
import type { ResourceUsage } from "@/types/monitor";

/**
 * Health of a single metric. Encoded as a word, not just a color, so the
 * state survives greyscale and color-vision deficiency - the dot is a
 * redundant cue, never the only one.
 */
type Health = "good" | "warn" | "bad" | "unknown";

const DOT_CLASS: Record<Health, string> = {
  good: "bg-emerald-500",
  warn: "bg-amber-500",
  bad: "bg-rose-500",
  unknown: "bg-muted-foreground/40",
};

const HEALTH_LABEL: Record<Health, string> = {
  good: "Healthy",
  warn: "Elevated",
  bad: "Critical",
  unknown: "Not measured",
};

interface Stat {
  key: string;
  label: string;
  /** The number itself, or null when the metric isn't available. */
  value: string | null;
  /** Context that earns its place - a limit, a total, a denominator. */
  detail?: string;
  health: Health;
}

/**
 * Thresholds, in one place so the dashboard and the instance page can't
 * disagree about what "healthy" means.
 *
 * These are judgement calls about Minecraft servers rather than universal
 * truths: 19+ TPS is imperceptible, below 15 is visibly stuttering; a JVM
 * sitting near its own `-Xmx` is about to spend its life in GC; and a disk
 * over 90% full is how a world corrupts on the next save.
 */
function pingHealth(ms: number): Health {
  if (ms <= 50) return "good";
  if (ms <= 150) return "warn";
  return "bad";
}

function tpsHealth(tps: number): Health {
  if (tps >= 19) return "good";
  if (tps >= 15) return "warn";
  return "bad";
}

function cpuHealth(percent: number): Health {
  if (percent < 70) return "good";
  if (percent < 90) return "warn";
  return "bad";
}

function ramHealth(usedMb: number, maxMb: number): Health {
  if (maxMb <= 0) return "unknown";
  const ratio = usedMb / maxMb;
  if (ratio < 0.75) return "good";
  if (ratio < 0.9) return "warn";
  return "bad";
}

function diskHealth(percent: number): Health {
  if (percent < 80) return "good";
  if (percent < 90) return "warn";
  return "bad";
}

/**
 * Builds the six metrics in a fixed order.
 *
 * The order is deliberate and never re-sorted by value: an operator learns
 * where each number sits and reads it by position. Network and TPS come
 * first because they're what players actually feel; CPU, RAM and disk are
 * the causes underneath; players is context for all of it.
 */
export function buildStats(usage: ResourceUsage, instance: Instance): Stat[] {
  const players = usage.ping?.playersOnline ?? usage.playersTracked;
  const maxPlayers = usage.ping?.playersMax ?? null;

  return [
    {
      key: "network",
      label: "Network",
      value: usage.ping ? `${usage.ping.latencyMs} ms` : null,
      detail: usage.ping ? undefined : "Server not answering yet",
      health: usage.ping ? pingHealth(usage.ping.latencyMs) : "unknown",
    },
    {
      key: "tps",
      label: "TPS",
      value: usage.tps !== null ? usage.tps.toFixed(1) : null,
      detail: usage.tps !== null ? "of 20.0" : "Not reported by this loader",
      health: usage.tps !== null ? tpsHealth(usage.tps) : "unknown",
    },
    {
      key: "cpu",
      label: "CPU",
      value: `${usage.cpuPercent.toFixed(0)}%`,
      health: cpuHealth(usage.cpuPercent),
    },
    {
      key: "ram",
      label: "RAM",
      value: formatMemoryMb(usage.memoryMb),
      detail: `of ${formatMemoryMb(instance.maxRamMb)} allocated`,
      health: ramHealth(usage.memoryMb, instance.maxRamMb),
    },
    {
      key: "disk",
      label: "Disk",
      value: usage.disk ? `${usage.disk.usedPercent.toFixed(0)}%` : null,
      detail: usage.disk ? `used on ${usage.disk.mountPoint}` : "Volume unavailable",
      health: usage.disk ? diskHealth(usage.disk.usedPercent) : "unknown",
    },
    {
      key: "players",
      label: "Players",
      value: String(players),
      detail: maxPlayers !== null ? `of ${maxPlayers} slots` : undefined,
      health: "good",
    },
  ];
}

function StatCard({ stat }: { stat: Stat }) {
  return (
    <div className="flex flex-col gap-1 rounded-xl border border-border bg-card p-3">
      <dt className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
        <span
          className={`inline-block size-2 shrink-0 rounded-full ${DOT_CLASS[stat.health]}`}
          role="img"
          aria-label={HEALTH_LABEL[stat.health]}
        />
        {stat.label}
      </dt>
      <dd className="flex flex-col gap-0.5">
        <span className="text-xl font-semibold tabular-nums">
          {stat.value ?? <span className="text-base font-normal text-muted-foreground">—</span>}
        </span>
        {stat.detail && (
          <span className="truncate text-xs font-normal text-muted-foreground" title={stat.detail}>
            {stat.detail}
          </span>
        )}
      </dd>
    </div>
  );
}

/**
 * The six-metric readout for one instance.
 *
 * Reads from the single app-wide resource poll (`useResourceUsagePolling`,
 * mounted once at the app root) rather than polling itself - with several of
 * these mounted at once, per-card polling would mean one backend round trip
 * per card per tick for data one shared call already covers.
 */
export function StatCards({ instance }: { instance: Instance }) {
  const usage = useResourceUsageStore((s) => s.usageByInstanceId[instance.id]);

  if (!usage || !usage.isRunning) {
    return <p className="text-xs text-muted-foreground">Waiting for resource data…</p>;
  }

  return (
    <dl className="grid grid-cols-2 gap-2 sm:grid-cols-3 lg:grid-cols-6">
      {buildStats(usage, instance).map((stat) => (
        <StatCard key={stat.key} stat={stat} />
      ))}
    </dl>
  );
}
