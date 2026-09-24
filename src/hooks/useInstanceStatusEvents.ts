import { useEffect } from "react";
import { useInstancesStore } from "@/stores/instancesStore";
import { STATUS_EVENT, type StatusChangedPayload } from "@/types/events";
import { listenWithCleanup } from "@/lib/tauri";

/**
 * Keeps the instances store in sync with server processes' actual
 * lifecycle, which changes on Rust's own schedule (a server finishing
 * startup, exiting, crashing) - not just in response to something the user
 * clicked here. Mount once near the app root.
 */
export function useInstanceStatusEvents() {
  const applyStatusUpdate = useInstancesStore((s) => s.applyStatusUpdate);

  useEffect(() => {
    return listenWithCleanup<StatusChangedPayload>(STATUS_EVENT, (event) => {
      applyStatusUpdate(event.payload.instanceId, event.payload.status);
    });
  }, [applyStatusUpdate]);
}
