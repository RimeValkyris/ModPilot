import { useEffect } from "react";
import {
  isPermissionGranted,
  requestPermission,
} from "@tauri-apps/plugin-notification";

/**
 * Requests OS notification permission once on startup, so the "server
 * finished starting" / "server crashed" notifications Rust sends actually
 * have permission to show by the time they're needed.
 */
export function useNotificationPermission() {
  useEffect(() => {
    isPermissionGranted().then((granted) => {
      if (!granted) requestPermission();
    });
  }, []);
}
