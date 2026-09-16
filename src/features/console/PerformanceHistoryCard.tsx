import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { TrendChart, type TrendSeries } from "@/features/dashboard/TrendChart";
import { formatMemoryMb } from "@/lib/format";
import { cn } from "@/lib/utils";
import { api } from "@/lib/tauri";
import type { Instance } from "@/types/instance";
import type { PerformanceSample } from "@/types/monitor";

/** Windows offered. Anything longer than a week is available through the
 * retention setting but not worth a preset - at one sample a minute the
 * chart stops resolving individual events well before then. */
const RANGES = [
  { hours: 6, label: "6h" },
  { hours: 24, label: "24h" },
  { hours: 24 * 7, label: "7d" },
] as const;

const CHARTS: {
  key: string;
  label: string;
  color: string;
  pick: (sample: PerformanceSample) => number | null;
  format: (value: number) => string;
  minTop: number;
}[] = [
  {
    key: "tps",
    label: "TPS",
    color: "var(--chart-1)",
    pick: (s) => s.tps,
    format: (v) => v.toFixed(1),
    // Pinned to the 20 ceiling: auto-scaling would make a drop from 20.0 to
    // 19.5 look like a collapse.
    minTop: 20,
  },
  {
    key: "mspt",
    label: "MSPT",
    color: "var(--chart-2)",
    pick: (s) => s.mspt,
    format: (v) => `${v.toFixed(1)} ms`,
    // 50 ms is a tick's budget, so the scale should always show it - the
    // whole question is how close to it the server runs.
    minTop: 50,
  },
  {
    key: "cpu",
    label: "CPU",
    color: "var(--chart-3)",
    pick: (s) => s.cpuPercent,
    format: (v) => `${Math.round(v)}%`,
    minTop: 100,
  },
  {
    key: "ram",
    label: "Memory",
    color: "var(--chart-4)",
    pick: (s) => s.memoryMb,
    format: (v) => formatMemoryMb(v),
    minTop: 1024,
  },
  {
    key: "players",
    label: "Players",
    color: "var(--chart-1)",
    pick: (s) => s.players,
    format: (v) => String(Math.round(v)),
    minTop: 4,
  },
];

/**
 * Recorded performance history, read from the database rather than from the
 * live poll's in-memory buffer.
 *
 * The distinction is the point: the buffer covers the last few minutes and
 * dies with the app, while this answers "when did this start?" and "was it
 * like this before the update?" — and works for a server that is currently
 * stopped, which is exactly when those questions get asked.
 */
export function PerformanceHistoryCard({ instance }: { instance: Instance }) {
  const [hours, setHours] = useState<number>(RANGES[1].hours);
  const [samples, setSamples] = useState<PerformanceSample[]>([]);
  const [isLoading, setIsLoading] = useState(true);

  const load = useCallback(
    async (window: number) => {
      setIsLoading(true);
      try {
        setSamples(await api.getPerformanceHistory(instance.id, window));
      } catch (err) {
        toast.error("Failed to load performance history", { description: String(err) });
      } finally {
        setIsLoading(false);
      }
    },
    [instance.id],
  );

  useEffect(() => {
    load(hours);
  }, [load, hours]);

  const timestamps = samples.map((s) => new Date(s.recordedAt).getTime());
  // Two points is the minimum for a line to mean anything.
  const canChart = samples.length >= 2;

  // A metric this server never reported is dropped rather than drawn as a
  // flat line at zero.
  const charts = CHARTS.map((chart) => {
    const values = samples.map(chart.pick);
    const series: TrendSeries[] = [
      { id: chart.key, label: chart.label, color: chart.color, values },
    ];
    return { ...chart, series, hasData: values.some((v) => v !== null) };
  }).filter((chart) => chart.hasData);

  return (
    <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-medium">Performance History</h2>
          <p className="text-xs text-muted-foreground">
            Recorded once a minute while this server runs, and kept locally.
          </p>
        </div>
        <div className="flex gap-1">
          {RANGES.map((range) => (
            <Button
              key={range.hours}
              size="sm"
              variant={hours === range.hours ? "secondary" : "ghost"}
              className={cn(hours === range.hours && "font-medium")}
              onClick={() => setHours(range.hours)}
            >
              {range.label}
            </Button>
          ))}
        </div>
      </div>

      {isLoading ? (
        <p className="text-sm text-muted-foreground">Loading…</p>
      ) : !canChart ? (
        <p className="text-sm text-muted-foreground">
          Not enough history yet. Samples are recorded once a minute while the
          server is running, so this fills in after a couple of minutes of
          uptime.
        </p>
      ) : (
        <div className="grid gap-3 lg:grid-cols-2">
          {charts.map((chart) => (
            <figure key={chart.key} className="rounded-lg border border-border p-3">
              <figcaption className="mb-1 text-xs font-medium">{chart.label}</figcaption>
              <TrendChart
                series={chart.series}
                timestamps={timestamps}
                format={chart.format}
                minTop={chart.minTop}
                height={130}
                /* Recorded history is sparse and its last sample may be
                   hours or days old, so the axis must not claim "now". */
                timeAxis="absolute"
              />
            </figure>
          ))}
        </div>
      )}
    </section>
  );
}
