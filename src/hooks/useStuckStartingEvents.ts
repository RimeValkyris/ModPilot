import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useInstancesStore } from "@/stores/instancesStore";
import {
  LOG_EVENT,
  STUCK_STARTING_EVENT,
  type LogLinePayload,
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

    return () => {
      unlistenStuck?.();
      unlistenLog?.();
    };
  }, [markInstanceStuck, clearInstanceStuck]);
}
