import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { toast } from "sonner";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { api } from "@/lib/tauri";

const CLOSE_REQUESTED_EVENT = "close-requested-with-running-servers";

/**
 * Listens for Rust's close-guard: if the window close button is pressed
 * while a server is still running, Rust holds the window open and emits
 * this event instead of just closing (which would orphan the Java
 * process). Offers to stop everything first rather than closing silently.
 */
export function CloseGuard() {
  const [open, setOpen] = useState(false);
  const [isStopping, setIsStopping] = useState(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen(CLOSE_REQUESTED_EVENT, () => setOpen(true)).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  async function handleStopAndExit() {
    setIsStopping(true);
    try {
      const runningIds = await api.listRunningInstanceIds();
      await Promise.all(runningIds.map((id) => api.forceStopInstance(id)));
      await api.quitApp();
    } catch (err) {
      toast.error("Failed to stop servers", { description: String(err) });
      setIsStopping(false);
    }
  }

  return (
    <AlertDialog open={open} onOpenChange={setOpen}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Servers are still running</AlertDialogTitle>
          <AlertDialogDescription>
            Closing ModpackPilot now would leave the Minecraft server process
            running with nothing managing it. Stop all running servers and
            exit, or cancel and stop them yourself first.
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={isStopping}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            className="bg-destructive text-white hover:bg-destructive/90"
            disabled={isStopping}
            onClick={handleStopAndExit}
          >
            {isStopping ? "Stopping…" : "Stop All & Exit"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
