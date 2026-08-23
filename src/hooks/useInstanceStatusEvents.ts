import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import { useInstancesStore } from "@/stores/instancesStore";
import { STATUS_EVENT, type StatusChangedPayload } from "@/types/events";

/**
 * Keeps the instances store in sync with server processes' actual
 * lifecycle, which changes on Rust's own schedule (a server finishing
 * startup, exiting, crashing) - not just in response to something the user
 * clicked here. Mount once near the app root.
 */
export function useInstanceStatusEvents() {
  const applyStatusUpdate = useInstancesStore((s) => s.applyStatusUpdate);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<StatusChangedPayload>(STATUS_EVENT, (event) => {
      applyStatusUpdate(event.payload.instanceId, event.payload.status);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [applyStatusUpdate]);
}
