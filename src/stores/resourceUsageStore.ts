import { create } from "zustand";
import { api } from "@/lib/tauri";
import type { DiskUsage, ResourceUsage } from "@/types/monitor";

/** One polled reading, kept so the dashboard can chart a trend rather than
 * only ever showing the latest instant. */
export interface UsageSample {
  t: number;
  cpuPercent: number;
  memoryMb: number;
  /** Null where the metric wasn't available at that tick, so a chart draws a
   * gap rather than a dip to zero. */
  diskPercent: number | null;
  pingMs: number | null;
  tps: number | null;
  mspt: number | null;
  players: number | null;
}

/** ~3 minutes of history at the 2s poll interval. Enough to see a trend
 * (a GC sawtooth, a startup ramp) without unbounded memory growth. */
const MAX_SAMPLES = 90;

/** Categorical color slots available to charts. Four, because that's what
 * the validated palette provides (`--chart-1..4`); anything beyond folds
 * into "Other" rather than inventing a 5th hue. */
export const CHART_SLOTS = 4;

interface ResourceUsageState {
  usageByInstanceId: Record<string, ResourceUsage>;
  /** Volume usage for the instances directory. Unlike the per-instance
   * metrics this stays meaningful with every server stopped, so it is
   * polled independently of whether anything is running. */
  disk: DiskUsage | null;
  historyByInstanceId: Record<string, UsageSample[]>;
  /** Stable per-instance color slot. Assigned on first sight and never
   * reshuffled while the instance stays present, so that stopping one
   * server never repaints the colors of the others. */
  colorSlotByInstanceId: Record<string, number>;
  poll: () => Promise<void>;
}

/** Guards against overlapping polls.
 *
 * A tick can outlast its own interval - each running instance's enrichment
 * includes a Server List Ping that waits out a timeout when a server is up
 * but not answering. Without this, those slow ticks would pile up and every
 * one of them would append another history sample, compressing the chart's
 * time base. Deliberately module-local rather than store state: it should
 * never trigger a re-render. */
let pollInFlight = false;

function assignSlots(
  existing: Record<string, number>,
  ids: string[],
): Record<string, number> {
  const next: Record<string, number> = {};
  // Keep every still-present instance on the slot it already had.
  for (const id of ids) {
    if (existing[id] !== undefined) next[id] = existing[id];
  }
  const taken = new Set(Object.values(next));
  for (const id of ids) {
    if (next[id] !== undefined) continue;
    let slot = 0;
    while (taken.has(slot) && slot < CHART_SLOTS) slot++;
    next[id] = slot % CHART_SLOTS;
    taken.add(next[id]);
  }
  return next;
}

/**
 * One shared poll for every running instance's resource usage, instead of
 * each instance card independently invoking its own command on its own
 * timer. With several servers running at once, that was N IPC round trips
 * every tick for data the backend can just as easily return in one.
 */
export const useResourceUsageStore = create<ResourceUsageState>((set, get) => ({
  usageByInstanceId: {},
  disk: null,
  historyByInstanceId: {},
  colorSlotByInstanceId: {},

  poll: async () => {
    if (pollInFlight) return;
    pollInFlight = true;
    try {
      const [usageByInstanceId, disk] = await Promise.all([
        api.getAllResourceUsage(),
        api.getDiskUsage(),
      ]);
      const ids = Object.keys(usageByInstanceId);
      const t = Date.now();

      const prev = get().historyByInstanceId;
      const historyByInstanceId: Record<string, UsageSample[]> = {};
      for (const id of ids) {
        const usage = usageByInstanceId[id];
        // Only ids in this tick are carried forward: a stopped server's
        // line should disappear, not linger as a stale flat trace.
        const series = [
          ...(prev[id] ?? []),
          {
            t,
            cpuPercent: usage.cpuPercent,
            memoryMb: usage.memoryMb,
            diskPercent: usage.disk?.usedPercent ?? null,
            pingMs: usage.ping?.latencyMs ?? null,
            tps: usage.tps,
            mspt: usage.mspt,
            // The ping is authoritative when it answered; the console roster
            // covers the window before the server accepts connections.
            players: usage.ping?.playersOnline ?? usage.playersTracked,
          },
        ];
        historyByInstanceId[id] =
          series.length > MAX_SAMPLES ? series.slice(series.length - MAX_SAMPLES) : series;
      }

      set({
        usageByInstanceId,
        disk,
        historyByInstanceId,
        colorSlotByInstanceId: assignSlots(get().colorSlotByInstanceId, ids),
      });
    } catch {
      // Transient errors aren't worth surfacing for a background poll -
      // the next tick will just try again.
    } finally {
      pollInFlight = false;
    }
  },
}));
