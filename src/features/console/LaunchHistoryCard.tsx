import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { listen } from "@tauri-apps/api/event";
import { AlertOctagon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/tauri";
import { formatUptime } from "@/lib/format";
import { CRASH_LOOP_EVENT, type CrashLoopPayload } from "@/types/events";
import type { Instance, LaunchHistoryEntry } from "@/types/instance";

const STATUS_BADGE: Record<LaunchHistoryEntry["status"], { className: string; label: string }> = {
  running: { className: "border-transparent bg-primary/15 text-primary", label: "Running" },
  stopped: { className: "border-transparent bg-muted text-muted-foreground", label: "Stopped" },
  crashed: {
    className: "border-transparent bg-destructive/15 text-destructive",
    label: "Crashed",
  },
};

/** How long a run lasted, or how long the current one has been going. */
function runDuration(entry: LaunchHistoryEntry): string {
  const start = new Date(entry.startedAt).getTime();
  const end = entry.stoppedAt ? new Date(entry.stoppedAt).getTime() : Date.now();
  return formatUptime(Math.max(0, Math.round((end - start) / 1000)));
}

/**
 * Every recorded run of this server, with how long it lasted and how it
 * ended.
 *
 * The `launch_history` table has been written since the first migration and
 * read by nothing — every exit code and crash timestamp was already being
 * recorded and never shown. The duration matters as much as the outcome: a
 * run that ended after twelve seconds failed during startup, one that ended
 * after nine hours failed under load, and those point at different causes.
 */
export function LaunchHistoryCard({ instance }: { instance: Instance }) {
  const [entries, setEntries] = useState<LaunchHistoryEntry[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [crashLoop, setCrashLoop] = useState<CrashLoopPayload | null>(null);

  const refresh = useCallback(async () => {
    setIsLoading(true);
    try {
      setEntries(await api.getLaunchHistory(instance.id));
    } catch (err) {
      toast.error("Failed to load launch history", { description: String(err) });
    } finally {
      setIsLoading(false);
    }
  }, [instance.id]);

  // Runs on mount and again whenever this instance's lifecycle changes, so
  // a crash that just happened appears without the operator reloading.
  useEffect(() => {
    refresh();
  }, [instance.status, refresh]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<CrashLoopPayload>(CRASH_LOOP_EVENT, (event) => {
      if (event.payload.instanceId !== instance.id) return;
      setCrashLoop(event.payload);
      refresh();
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [instance.id, refresh]);

  // A fresh start means whatever loop there was is over.
  useEffect(() => {
    if (instance.status === "running") setCrashLoop(null);
  }, [instance.status]);

  return (
    <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
      <div>
        <h2 className="text-sm font-medium">Launch History</h2>
        <p className="text-xs text-muted-foreground">
          How each run of this server ended, and how long it lasted.
        </p>
      </div>

      {crashLoop && (
        <div className="flex items-start gap-2.5 rounded-lg border border-destructive/40 bg-destructive/10 p-3">
          <AlertOctagon className="mt-0.5 size-4 shrink-0 text-destructive" />
          <div>
            <p className="text-sm font-medium text-destructive">Crash loop detected</p>
            <p className="mt-0.5 text-xs text-muted-foreground">
              Auto-restart gave up after {crashLoop.crashCount} consecutive
              crashes. A restart isn't going to fix this on its own — run
              Diagnostics above, and check the modpack report, before starting
              it again.
            </p>
          </div>
        </div>
      )}

      {isLoading ? (
        <p className="text-sm text-muted-foreground">Loading…</p>
      ) : entries.length === 0 ? (
        <p className="text-sm text-muted-foreground">
          This server hasn't been started yet.
        </p>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {entries.map((entry) => (
            <li
              key={entry.id}
              className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border p-2.5 text-sm"
            >
              <div className="flex items-center gap-2">
                <Badge className={STATUS_BADGE[entry.status].className}>
                  {STATUS_BADGE[entry.status].label}
                </Badge>
                <span className="text-muted-foreground">
                  {new Date(entry.startedAt).toLocaleString()}
                </span>
              </div>
              <div className="flex items-center gap-3 text-xs text-muted-foreground">
                <span>Ran {runDuration(entry)}</span>
                {entry.exitCode !== null && (
                  <span className="font-mono">exit {entry.exitCode}</span>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
