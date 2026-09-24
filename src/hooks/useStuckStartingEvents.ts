import { useEffect } from "react";
import { toast } from "sonner";
import { useInstancesStore } from "@/stores/instancesStore";
import { listenWithCleanup } from "@/lib/tauri";
import {
  CRASH_LOOP_EVENT,
  LOG_EVENT,
  RESOURCE_ALERT_EVENT,
  STUCK_STARTING_EVENT,
  type CrashLoopPayload,
  type LogLinePayload,
  type ResourceAlertPayload,
  type StuckStartingPayload,
} from "@/types/events";

/**
 * Keeps `stuckInstanceIds` in sync with the backend's own startup watchdog
 * (see `server::process::watch_for_stuck_startup`) - global and driven by
 * the actual child process's real activity, not by whichever console tab
 * happens to be open. Mount once near the app root, same as
 * `useInstanceStatusEvents`.
 */
export function useStuckStartingEvents() {
  const markInstanceStuck = useInstancesStore((s) => s.markInstanceStuck);
  const clearInstanceStuck = useInstancesStore((s) => s.clearInstanceStuck);

  useEffect(() => {
    const cleanupStuck = listenWithCleanup<StuckStartingPayload>(STUCK_STARTING_EVENT, (event) => {
      markInstanceStuck(event.payload.instanceId);
    });

    // Any new output means it's not stuck (anymore) - clears the flag if
    // the instance recovers on its own after being flagged.
    const cleanupLog = listenWithCleanup<LogLinePayload>(LOG_EVENT, (event) => {
      clearInstanceStuck(event.payload.instanceId);
    });

    // Sustained resource problems (see Rust's `server::alerts`) surface as
    // a toast in-app; the backend also raises an OS notification so it is
    // seen even when ModpackPilot is not focused.
    const cleanupAlert = listenWithCleanup<ResourceAlertPayload>(RESOURCE_ALERT_EVENT, (event) => {
      toast.warning("Resource alert", { description: event.payload.message });
    });

    // Auto-restart giving up is the most consequential thing that can
    // happen unattended - the server is down and staying down - so it is
    // surfaced wherever the operator happens to be, not only on the
    // instance's own page. Persistent, because a toast that auto-dismisses
    // is exactly as good as no toast for something discovered later.
    const cleanupCrashLoop = listenWithCleanup<CrashLoopPayload>(CRASH_LOOP_EVENT, (event) => {
      toast.error(`${event.payload.instanceName}: crash loop detected`, {
        description: `Auto-restart gave up after ${event.payload.crashCount} consecutive crashes. Open its Diagnostics tab to see why.`,
        duration: Infinity,
        closeButton: true,
      });
    });

    return () => {
      cleanupStuck();
      cleanupLog();
      cleanupAlert();
      cleanupCrashLoop();
    };
  }, [markInstanceStuck, clearInstanceStuck]);
}
