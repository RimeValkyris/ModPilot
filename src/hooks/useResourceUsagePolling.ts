import { useEffect } from "react";
import { useInstancesStore } from "@/stores/instancesStore";
import { useResourceUsageStore } from "@/stores/resourceUsageStore";

const POLL_INTERVAL_MS = 2000;

/**
 * Drives the single shared resource-usage poll, active only while at least
 * one instance is actually running - no point asking the backend to sample
 * processes that don't exist. Mount once near the app root.
 */
export function useResourceUsagePolling() {
  const hasRunningInstance = useInstancesStore((s) =>
    s.instances.some((i) => i.status === "running"),
  );
  const poll = useResourceUsageStore((s) => s.poll);

  useEffect(() => {
    if (!hasRunningInstance) return;

    poll();
    const interval = setInterval(poll, POLL_INTERVAL_MS);
    return () => clearInterval(interval);
  }, [hasRunningInstance, poll]);
}
