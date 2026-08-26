import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { useInstancesStore } from "@/stores/instancesStore";
import {
  LOG_EVENT,
  RESOURCE_ALERT_EVENT,
  STUCK_STARTING_EVENT,
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
    let unlistenStuck: (() => void) | undefined;
    let unlistenLog: (() => void) | undefined;

    listen<StuckStartingPayload>(STUCK_STARTING_EVENT, (event) => {
      markInstanceStuck(event.payload.instanceId);
    }).then((fn) => {
      unlistenStuck = fn;
    });

    // Any new output means it's not stuck (anymore) - clears the flag if
    // the instance recovers on its own after being flagged.
    listen<LogLinePayload>(LOG_EVENT, (event) => {
      clearInstanceStuck(event.payload.instanceId);
    }).then((fn) => {
      unlistenLog = fn;
    });

    // Sustained resource problems (see Rust's `server::alerts`) surface as
    // a toast in-app; the backend also raises an OS notification so it is
    // seen even when ModpackPilot is not focused.
    let unlistenAlert: (() => void) | undefined;
    listen<ResourceAlertPayload>(RESOURCE_ALERT_EVENT, (event) => {
      toast.warning("Resource alert", { description: event.payload.message });
    }).then((fn) => {
      unlistenAlert = fn;
    });

    return () => {
      unlistenStuck?.();
      unlistenLog?.();
      unlistenAlert?.();
    };
  }, [markInstanceStuck, clearInstanceStuck]);
}
