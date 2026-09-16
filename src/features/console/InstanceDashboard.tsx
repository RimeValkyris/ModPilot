import { useResourceUsageStore, type UsageSample } from "@/stores/resourceUsageStore";
import { formatMemoryMb, formatUptime } from "@/lib/format";
import { StatCards } from "@/features/dashboard/StatCards";
import { TrendChart, type TrendSeries } from "@/features/dashboard/TrendChart";
import { Badge } from "@/components/ui/badge";
import type { Instance } from "@/types/instance";
import type { HealthStatus } from "@/types/monitor";

/** Tinted badge classes per verdict, following the same idiom as
 * `STATUS_BADGE_CLASS`. `unknown` never renders - a stopped server shows no
 * verdict rather than a grey one. */
const HEALTH_BADGE: Record<HealthStatus, { className: string; label: string }> = {
  healthy: { className: "border-transparent bg-primary/15 text-primary", label: "HEALTHY" },
  warning: {
    className: "border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-400",
    label: "WARNING",
  },
  critical: {
    className: "border-transparent bg-destructive/15 text-destructive",
    label: "CRITICAL",
  },
  unknown: { className: "border-transparent bg-muted text-muted-foreground", label: "UNKNOWN" },
};

/** One chart per metric, in the same order as the cards above them, so the
 * eye can travel straight down from a number to its history. */
const CHARTS: {
  key: string;
  label: string;
  color: string;
  pick: (sample: UsageSample) => number | null;
  format: (value: number) => string;
  minTop: number;
}[] = [
  {
    key: "tps",
    label: "TPS",
    color: "var(--chart-1)",
    pick: (s) => s.tps,
    format: (v) => v.toFixed(1),
    // Pinned to 20: TPS is meaningful only against its ceiling, and
    // auto-scaling would make a server dropping from 20 to 19 look like a
    // collapse.
    minTop: 20,
  },
  {
    key: "mspt",
    label: "MSPT",
    color: "var(--chart-5)",
    pick: (s) => s.mspt,
    format: (v) => `${v.toFixed(1)} ms`,
    // 50 ms is the tick budget; the scale should always show it, since the
    // question is how much headroom is left rather than the raw number.
    minTop: 50,
  },
  {
    key: "cpu",
    label: "CPU",
    color: "var(--chart-2)",
    pick: (s) => s.cpuPercent,
    format: (v) => `${Math.round(v)}%`,
    minTop: 100,
  },
  {
    key: "ram",
    label: "Memory",
    color: "var(--chart-3)",
    pick: (s) => s.memoryMb,
    format: (v) => formatMemoryMb(v),
    minTop: 1024,
  },
  {
    key: "ping",
    label: "Network",
    color: "var(--chart-4)",
    pick: (s) => s.pingMs,
    format: (v) => `${Math.round(v)} ms`,
    minTop: 100,
  },
];

/**
 * The per-server dashboard shown on an instance's Overview tab: the six
 * headline metrics, then a chart for each one that has a history worth
 * drawing.
 *
 * Charts are one metric each rather than one chart with several y-scales.
 * TPS, percentages, megabytes and milliseconds share no domain, and a dual
 * axis invites the reader to see a correlation that the scaling invented.
 *
 * A metric the server never reported (TPS on a loader without the command,
 * ping before the server accepts connections) is dropped entirely rather
 * than drawn as a flat line at zero.
 */
export function InstanceDashboard({ instance }: { instance: Instance }) {
  const usage = useResourceUsageStore((s) => s.usageByInstanceId[instance.id]);
  const history = useResourceUsageStore((s) => s.historyByInstanceId[instance.id]);

  const samples = history ?? [];
  const timestamps = samples.map((s) => s.t);
  // Two points is the minimum for a line to mean anything.
  const canChart = samples.length >= 2;

  const charts = CHARTS.map((chart) => {
    const values = samples.map(chart.pick);
    const hasData = values.some((v) => v !== null);
    const series: TrendSeries[] = [
      { id: chart.key, label: chart.label, color: chart.color, values },
    ];
    return { ...chart, series, hasData };
  }).filter((chart) => chart.hasData);

  return (
    <section className="flex flex-col gap-4">
      <div className="flex flex-wrap items-baseline justify-between gap-3">
        <div className="flex items-center gap-2">
          <h2 className="text-sm font-medium text-muted-foreground">Live metrics</h2>
          {usage?.isRunning && usage.health.status !== "unknown" && (
            <Badge className={HEALTH_BADGE[usage.health.status].className}>
              {HEALTH_BADGE[usage.health.status].label}
            </Badge>
          )}
        </div>
        {usage?.isRunning && (
          <span className="text-xs tabular-nums text-muted-foreground">
            Up {formatUptime(usage.uptimeSeconds)}
          </span>
        )}
      </div>

      {/* A verdict is never shown without what produced it. */}
      {usage?.isRunning && usage.health.reasons.length > 0 && (
        <ul className="flex flex-col gap-0.5 rounded-lg border border-border bg-card p-3 text-xs text-muted-foreground">
          {usage.health.reasons.map((reason) => (
            <li key={reason}>{reason}</li>
          ))}
        </ul>
      )}

      <StatCards instance={instance} />

      {canChart && charts.length > 0 && (
        <div className="grid gap-3 lg:grid-cols-2">
          {charts.map((chart) => (
            <figure key={chart.key} className="rounded-xl border border-border bg-card p-3">
              <figcaption className="mb-1 text-xs font-medium">{chart.label}</figcaption>
              <TrendChart
                series={chart.series}
                timestamps={timestamps}
                format={chart.format}
                minTop={chart.minTop}
                height={130}
              />
            </figure>
          ))}
        </div>
      )}
    </section>
  );
}
