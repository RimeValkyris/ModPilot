import { useState } from "react";
import { Table2, LineChart as LineChartIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  useResourceUsageStore,
  CHART_SLOTS,
  type UsageSample,
} from "@/stores/resourceUsageStore";
import { formatMemoryMb } from "@/lib/format";
import { TrendChart, type TrendSeries } from "@/features/dashboard/TrendChart";
import type { Instance } from "@/types/instance";

/** Fixed categorical order - assigned by slot, never cycled past the end.
 * A 5th running server folds into "Other" rather than getting an invented
 * hue that nobody could distinguish under CVD. */
const SLOT_COLORS = [
  "var(--chart-1)",
  "var(--chart-2)",
  "var(--chart-3)",
  "var(--chart-4)",
];
const OTHER_COLOR = "var(--chart-5)";

/** Wall-clock span covered by a series of sample timestamps. */
function formatSpan(timestamps: number[]): string {
  if (timestamps.length < 2) return "moments";
  const seconds = Math.round((timestamps[timestamps.length - 1] - timestamps[0]) / 1000);
  if (seconds < 90) return `${seconds}s`;
  return `${Math.round(seconds / 60)}m`;
}

export function ResourceTrends({ instances }: { instances: Instance[] }) {
  const historyByInstanceId = useResourceUsageStore((s) => s.historyByInstanceId);
  const colorSlotByInstanceId = useResourceUsageStore((s) => s.colorSlotByInstanceId);
  const [showTable, setShowTable] = useState(false);

  const running = instances.filter(
    (i) => i.status === "running" && (historyByInstanceId[i.id]?.length ?? 0) > 0,
  );

  // Need at least two samples before a "trend" means anything.
  const longest = Math.max(0, ...running.map((i) => historyByInstanceId[i.id]?.length ?? 0));
  if (running.length === 0 || longest < 2) return null;

  // Shared x-axis from the longest history; shorter series left-pad with
  // nulls so every line stays on the same time base.
  const reference = running.reduce((best, i) =>
    (historyByInstanceId[i.id]?.length ?? 0) > (historyByInstanceId[best.id]?.length ?? 0)
      ? i
      : best,
  );
  const timestamps = historyByInstanceId[reference.id].map((s) => s.t);

  // `pick` may return null for a metric a given tick had no reading for -
  // the chart draws a gap there rather than a dip to zero.
  function build(pick: (s: UsageSample) => number | null): TrendSeries[] {
    const named = running.map((instance) => {
      const samples = historyByInstanceId[instance.id] ?? [];
      const pad = timestamps.length - samples.length;
      const slot = colorSlotByInstanceId[instance.id] ?? 0;
      return {
        id: instance.id,
        label: instance.name,
        color: slot < CHART_SLOTS ? SLOT_COLORS[slot] : OTHER_COLOR,
        values: [...Array<number | null>(Math.max(0, pad)).fill(null), ...samples.map(pick)],
      };
    });
    return named;
  }

  const cpuSeries = build((s) => s.cpuPercent);
  const ramSeries = build((s) => s.memoryMb);
  const diskSeries = build((s) => s.diskPercent);

  // Several instances on the same volume report the same disk number, so the
  // chart is only worth its space when at least one series has readings.
  const hasDisk = diskSeries.some((s) => s.values.some((v) => v !== null));

  return (
    <section className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        {/* Derived from the timestamps themselves, not from sample count x
            interval: the poll slows to a crawl while nothing is running, so
            a fixed 2s-per-sample assumption would mislabel any history that
            spans a start-up. */}
        <h2 className="text-sm font-medium text-muted-foreground">
          Resource Usage · last {formatSpan(timestamps)}
        </h2>
        {/* Table view keeps every value reachable without hovering. */}
        <Button variant="ghost" size="sm" onClick={() => setShowTable((v) => !v)}>
          {showTable ? <LineChartIcon /> : <Table2 />}
          {showTable ? "Charts" : "Table"}
        </Button>
      </div>

      {/* Legend: always present for 2+ series, so identity never rests on
          color alone. A single series is named by the chart title instead. */}
      {running.length > 1 && (
        <ul className="flex flex-wrap gap-x-4 gap-y-1">
          {cpuSeries.map((s) => (
            <li key={s.id} className="flex items-center gap-1.5 text-xs text-muted-foreground">
              <span
                className="inline-block h-0.5 w-3 rounded-full"
                style={{ backgroundColor: s.color }}
              />
              {s.label}
            </li>
          ))}
        </ul>
      )}

      {showTable ? (
        <div className="overflow-x-auto rounded-xl border border-border bg-card">
          <table className="w-full text-sm">
            <caption className="sr-only">
              Latest CPU, memory and disk readings per running server
            </caption>
            <thead>
              <tr className="border-b border-border text-left text-xs text-muted-foreground">
                <th scope="col" className="p-2 font-medium">Server</th>
                <th scope="col" className="p-2 font-medium">CPU</th>
                <th scope="col" className="p-2 font-medium">Memory</th>
                <th scope="col" className="p-2 font-medium">Disk</th>
                <th scope="col" className="p-2 font-medium">Peak CPU</th>
                <th scope="col" className="p-2 font-medium">Peak memory</th>
              </tr>
            </thead>
            <tbody>
              {running.map((instance) => {
                const samples = historyByInstanceId[instance.id] ?? [];
                const last = samples[samples.length - 1];
                const peakCpu = Math.max(...samples.map((s) => s.cpuPercent));
                const peakMem = Math.max(...samples.map((s) => s.memoryMb));
                return (
                  <tr key={instance.id} className="border-b border-border last:border-0">
                    <th scope="row" className="p-2 text-left font-normal">{instance.name}</th>
                    <td className="p-2">{last.cpuPercent.toFixed(0)}%</td>
                    <td className="p-2">{formatMemoryMb(last.memoryMb)}</td>
                    <td className="p-2">
                      {last.diskPercent !== null ? `${last.diskPercent.toFixed(0)}%` : "—"}
                    </td>
                    <td className="p-2">{peakCpu.toFixed(0)}%</td>
                    <td className="p-2">{formatMemoryMb(peakMem)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      ) : (
        // Two charts, never one with two y-scales: CPU% and megabytes don't
        // share a domain, and a dual axis lets the reader invent whatever
        // correlation the scaling happens to suggest.
        <div className="grid gap-4 lg:grid-cols-2">
          <figure className="rounded-xl border border-border bg-card p-3">
            <figcaption className="mb-1 text-xs font-medium">CPU</figcaption>
            <TrendChart
              series={cpuSeries}
              timestamps={timestamps}
              format={(v) => `${Math.round(v)}%`}
              minTop={100}
            />
          </figure>
          <figure className="rounded-xl border border-border bg-card p-3">
            <figcaption className="mb-1 text-xs font-medium">Memory</figcaption>
            <TrendChart
              series={ramSeries}
              timestamps={timestamps}
              format={(v) => formatMemoryMb(v)}
              minTop={1024}
            />
          </figure>
          {hasDisk && (
            <figure className="rounded-xl border border-border bg-card p-3">
              <figcaption className="mb-1 text-xs font-medium">
                Disk <span className="font-normal text-muted-foreground">· volume used</span>
              </figcaption>
              {/* Pinned to 100: disk usage only means anything against the
                  full volume, and auto-scaling a 40-42% range would turn a
                  flat two hours into an alarming climb. */}
              <TrendChart
                series={diskSeries}
                timestamps={timestamps}
                format={(v) => `${Math.round(v)}%`}
                minTop={100}
              />
            </figure>
          )}
        </div>
      )}
    </section>
  );
}
