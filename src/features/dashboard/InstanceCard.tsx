import { useEffect } from "react";
import { useState } from "react";
import { toast } from "sonner";
import { MoreVertical, OctagonX, Pencil, Play, RotateCw, Square, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
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
import { useInstances } from "@/hooks/useInstances";
import { useJavaStore } from "@/stores/javaStore";
import { api } from "@/lib/tauri";
import type { Instance, ServerStatus } from "@/types/instance";

const NO_JAVA_VALUE = "__none__";

const STATUS_LABEL: Record<ServerStatus, string> = {
  stopped: "STOPPED",
  starting: "STARTING",
  running: "RUNNING",
  stopping: "STOPPING",
  crashed: "CRASHED",
};

const STATUS_DOT: Record<ServerStatus, string> = {
  stopped: "bg-muted-foreground",
  starting: "bg-yellow-500",
  running: "bg-green-500",
  stopping: "bg-yellow-500",
  crashed: "bg-destructive",
};

function notImplemented(feature: string) {
  toast.info(`${feature} isn't implemented yet`, {
    description: "This will be wired up in an upcoming phase.",
  });
}

export function InstanceCard({ instance }: { instance: Instance }) {
  const { renameInstance, deleteInstance, setInstanceJava } = useInstances();
  const { installations, fetchInstallations } = useJavaStore();
  const [renameOpen, setRenameOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [nameDraft, setNameDraft] = useState(instance.name);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isProcessActionPending, setIsProcessActionPending] = useState(false);

  const canDelete = instance.status === "stopped" || instance.status === "crashed";

  useEffect(() => {
    fetchInstallations();
  }, [fetchInstallations]);

  async function runProcessAction(action: () => Promise<void>, failureMessage: string) {
    setIsProcessActionPending(true);
    try {
      await action();
    } catch (err) {
      toast.error(failureMessage, { description: String(err) });
    } finally {
      setIsProcessActionPending(false);
    }
  }

  async function handleJavaChange(value: string | null) {
    try {
      await setInstanceJava(instance.id, value && value !== NO_JAVA_VALUE ? value : null);
    } catch (err) {
      toast.error("Failed to set Java", { description: String(err) });
    }
  }

  async function handleRename(e: React.FormEvent) {
    e.preventDefault();
    if (!nameDraft.trim() || nameDraft.trim() === instance.name) {
      setRenameOpen(false);
      return;
    }
    setIsSubmitting(true);
    try {
      await renameInstance(instance.id, nameDraft.trim());
      toast.success("Instance renamed");
      setRenameOpen(false);
    } catch (err) {
      toast.error("Failed to rename instance", { description: String(err) });
    } finally {
      setIsSubmitting(false);
    }
  }

  async function handleDelete() {
    setIsSubmitting(true);
    try {
      await deleteInstance(instance.id);
      toast.success(`Deleted "${instance.name}"`);
      setDeleteOpen(false);
    } catch (err) {
      toast.error("Failed to delete instance", { description: String(err) });
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <li className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
      <div className="flex items-start justify-between gap-2">
        <div>
          <p className="font-medium">{instance.name}</p>
          <p className="text-sm text-muted-foreground">
            {instance.minecraftVersion ?? "Unknown version"} · {instance.loader}
          </p>
        </div>
        <DropdownMenu>
          <DropdownMenuTrigger render={<Button variant="ghost" size="icon-sm" />}>
            <MoreVertical />
          </DropdownMenuTrigger>
          <DropdownMenuContent align="end">
            <DropdownMenuItem onClick={() => setRenameOpen(true)}>
              <Pencil />
              Rename
            </DropdownMenuItem>
            {(instance.status === "running" || instance.status === "stopping") && (
              <DropdownMenuItem
                variant="destructive"
                onClick={() =>
                  runProcessAction(
                    () => api.forceStopInstance(instance.id),
                    "Failed to force stop instance",
                  )
                }
              >
                <OctagonX />
                Force Stop
              </DropdownMenuItem>
            )}
            <DropdownMenuItem
              variant="destructive"
              disabled={!canDelete}
              onClick={() => setDeleteOpen(true)}
            >
              <Trash2 />
              Delete
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>
      </div>

      <div className="flex items-center gap-1.5">
        <span className={`size-2 rounded-full ${STATUS_DOT[instance.status]}`} />
        <Badge variant="outline">{STATUS_LABEL[instance.status]}</Badge>
      </div>

      <Select
        value={instance.javaInstallationId ?? NO_JAVA_VALUE}
        onValueChange={handleJavaChange}
      >
        <SelectTrigger className="w-full" size="sm">
          <SelectValue placeholder="No Java selected" />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value={NO_JAVA_VALUE}>No Java selected</SelectItem>
          {installations.map((java) => (
            <SelectItem key={java.id} value={java.id}>
              Java {java.version} ({java.architecture})
            </SelectItem>
          ))}
        </SelectContent>
      </Select>

      <div className="flex gap-2">
        {(instance.status === "stopped" || instance.status === "crashed") && (
          <Button
            size="sm"
            className="flex-1"
            disabled={isProcessActionPending}
            onClick={() =>
              runProcessAction(() => api.startInstance(instance.id), "Failed to start instance")
            }
          >
            <Play />
            Start
          </Button>
        )}
        {instance.status === "starting" && (
          <Button size="sm" className="flex-1" disabled>
            Starting…
          </Button>
        )}
        {instance.status === "running" && (
          <>
            <Button
              variant="outline"
              size="sm"
              className="flex-1"
              disabled={isProcessActionPending}
              onClick={() =>
                runProcessAction(() => api.stopInstance(instance.id), "Failed to stop instance")
              }
            >
              <Square />
              Stop
            </Button>
            <Button
              variant="outline"
              size="sm"
              className="flex-1"
              disabled={isProcessActionPending}
              onClick={() =>
                runProcessAction(
                  () => api.restartInstance(instance.id),
                  "Failed to restart instance",
                )
              }
            >
              <RotateCw />
              Restart
            </Button>
          </>
        )}
        {instance.status === "stopping" && (
          <Button variant="outline" size="sm" className="flex-1" disabled>
            Stopping…
          </Button>
        )}
      </div>

      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          className="flex-1"
          onClick={() => notImplemented("Console")}
        >
          Console
        </Button>
        <Button
          variant="outline"
          size="sm"
          className="flex-1"
          onClick={() => notImplemented("Manage")}
        >
          Manage
        </Button>
      </div>

      <Dialog open={renameOpen} onOpenChange={setRenameOpen}>
        <DialogContent>
          <form onSubmit={handleRename} className="flex flex-col gap-4">
            <DialogHeader>
              <DialogTitle>Rename instance</DialogTitle>
              <DialogDescription>
                This only changes the display name - the instance folder on
                disk stays the same.
              </DialogDescription>
            </DialogHeader>
            <Input
              autoFocus
              value={nameDraft}
              onChange={(e) => setNameDraft(e.target.value)}
            />
            <DialogFooter>
              <Button type="submit" disabled={isSubmitting}>
                {isSubmitting ? "Saving…" : "Save"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <AlertDialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete "{instance.name}"?</AlertDialogTitle>
            <AlertDialogDescription>
              This permanently deletes the instance's server files, world,
              mods, and logs from disk. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              disabled={isSubmitting}
              onClick={handleDelete}
            >
              {isSubmitting ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </li>
  );
}
