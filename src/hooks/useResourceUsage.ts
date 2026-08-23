import { useEffect, useState } from "react";
import { api } from "@/lib/tauri";
import type { ResourceUsage } from "@/types/monitor";

const POLL_INTERVAL_MS = 2000;

/**
 * Polls an instance's resource usage while `enabled` (i.e. while it's
 * actually running and something is showing this data) - no point asking
 * the backend to sample a process that isn't there, or when nothing is
 * rendering the result.
 */
export function useResourceUsage(id: string, enabled: boolean) {
  const [usage, setUsage] = useState<ResourceUsage | null>(null);

  useEffect(() => {
    if (!enabled) {
      setUsage(null);
      return;
    }

    let cancelled = false;
    async function poll() {
      try {
        const result = await api.getResourceUsage(id);
        if (!cancelled) setUsage(result);
      } catch {
        // Transient errors (e.g. the process just exited) aren't worth
        // surfacing here - the next status event will update the UI.
      }
    }

    poll();
    const interval = setInterval(poll, POLL_INTERVAL_MS);
    return () => {
      cancelled = true;
      clearInterval(interval);
    };
  }, [id, enabled]);

  return usage;
}
