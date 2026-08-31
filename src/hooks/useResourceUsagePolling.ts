import { useEffect } from "react";
import { useInstancesStore } from "@/stores/instancesStore";
import { useResourceUsageStore } from "@/stores/resourceUsageStore";

/** Fast enough for a CPU trace to look live while a server is running. */
const ACTIVE_INTERVAL_MS = 2000;

/**
 * Idle cadence, for when nothing is running.
 *
 * The poll still has work to do with every server stopped - disk usage is a
 * property of the machine, and the dashboard shows it either way - but
 * free space moves slowly and the per-instance sampling short-circuits, so
 * there is nothing to gain from asking every two seconds.
 */
const IDLE_INTERVAL_MS = 30000;

/**
 * Drives the single shared resource-usage poll. Mount once near the app root.
 *
 * Always polls, but slows right down when no instance is running: the
 * process sampling has nothing to measure then, while the disk reading is
 * still worth keeping current.
 */
export function useResourceUsagePolling() {
  const hasRunningInstance = useInstancesStore((s) =>
    s.instances.some((i) => i.status === "running"),
  );
  const poll = useResourceUsageStore((s) => s.poll);

  useEffect(() => {
    poll();
    const interval = setInterval(
      poll,
      hasRunningInstance ? ACTIVE_INTERVAL_MS : IDLE_INTERVAL_MS,
    );
    return () => clearInterval(interval);
  }, [hasRunningInstance, poll]);
}
