import { useEffect } from "react";
import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { toast } from "sonner";
import { Copy, MoreVertical, OctagonX, Pencil, Play, RotateCw, Square, Trash2 } from "lucide-react";
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
import { useInstancesStore } from "@/stores/instancesStore";
import { useJavaStore } from "@/stores/javaStore";
import { useInstanceAvatar } from "@/hooks/useInstanceAvatar";
import { ResourceUsageRow } from "@/features/dashboard/ResourceUsageRow";
import { api } from "@/lib/tauri";
import { STATUS_BADGE_CLASS, STATUS_LABEL } from "@/lib/serverStatus";
import { ServerAvatarPlaceholder } from "@/components/ServerAvatarPlaceholder";
import { needsForgeInstall } from "@/features/console/ForgeInstallBanner";
import { getRequiredJavaMajor, parseJavaMajor } from "@/lib/javaRequirement";
import type { Instance } from "@/types/instance";

const NO_JAVA_VALUE = "__none__";

export function InstanceCard({ instance }: { instance: Instance }) {
  const navigate = useNavigate();
  // Store directly (not the `useInstances()` wrapper): this component only
  // needs mutation actions, and with one InstanceCard per server, going
  // through the fetch-on-mount wrapper would fire a redundant list_instances
  // call per card every time the dashboard renders.
  const { renameInstance, duplicateInstance, deleteInstance, setInstanceJava } =
    useInstancesStore();
  const isStuckStarting = useInstancesStore((s) => s.stuckInstanceIds.has(instance.id));
  const { installations, fetchInstallations } = useJavaStore();
  const [renameOpen, setRenameOpen] = useState(false);
  const [duplicateOpen, setDuplicateOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [forceStopOpen, setForceStopOpen] = useState(false);
  const [nameDraft, setNameDraft] = useState(instance.name);
  const [duplicateNameDraft, setDuplicateNameDraft] = useState(`${instance.name} (Copy)`);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [isProcessActionPending, setIsProcessActionPending] = useState(false);
  const [avatar] = useInstanceAvatar(instance.id);

  const canDelete = instance.status === "stopped" || instance.status === "crashed";

  const requiredJava = getRequiredJavaMajor(instance.minecraftVersion);
  const assignedJava = installations.find((j) => j.id === instance.javaInstallationId);
  const assignedJavaMajor = assignedJava ? parseJavaMajor(assignedJava.version) : null;
  const javaMismatch =
    requiredJava !== null && assignedJavaMajor !== null && assignedJavaMajor !== requiredJava;

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

  async function handleDuplicate(e: React.FormEvent) {
    e.preventDefault();
    if (!duplicateNameDraft.trim()) return;
    setIsSubmitting(true);
    try {
      const copy = await duplicateInstance(instance.id, duplicateNameDraft.trim());
      toast.success(`Created "${copy.name}"`);
      setDuplicateOpen(false);
    } catch (err) {
      toast.error("Failed to duplicate instance", { description: String(err) });
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

  async function handleForceStop() {
    setIsProcessActionPending(true);
    try {
      await api.forceStopInstance(instance.id);
      setForceStopOpen(false);
    } catch (err) {
      toast.error("Failed to force stop instance", { description: String(err) });
    } finally {
      setIsProcessActionPending(false);
    }
  }

  return (
    <li className="relative flex flex-col gap-3 overflow-hidden rounded-2xl border border-border bg-card p-3 shadow-sm transition-shadow hover:shadow-md">
      {/* Large, poster-style icon area - the server's identity comes first,
          everything else (status, actions, controls) is layered on or
          stacked below it rather than competing for space in a header row. */}
      <div className="relative aspect-square w-full overflow-hidden rounded-xl">
        {avatar ? (
          <img src={avatar} alt="" className="size-full object-cover" />
        ) : (
          <ServerAvatarPlaceholder className="size-full" />
        )}

        <div className="absolute inset-x-2 top-2 flex items-start justify-between gap-2">
          <Badge
            className={`${
              isStuckStarting
                ? "border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-400"
                : STATUS_BADGE_CLASS[instance.status]
            } shadow-sm backdrop-blur-sm`}
          >
            {isStuckStarting ? "POSSIBLY STUCK" : STATUS_LABEL[instance.status]}
          </Badge>
          <DropdownMenu>
            <DropdownMenuTrigger
              render={
                <Button
                  variant="ghost"
                  size="icon-sm"
                  className="bg-black/40 text-white hover:bg-black/60"
                />
              }
            >
              <MoreVertical />
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              <DropdownMenuItem onClick={() => setRenameOpen(true)}>
                <Pencil />
                Rename
              </DropdownMenuItem>
              <DropdownMenuItem onClick={() => setDuplicateOpen(true)}>
                <Copy />
                Duplicate
              </DropdownMenuItem>
              {(instance.status === "running" ||
                instance.status === "stopping" ||
                instance.status === "starting") && (
                <DropdownMenuItem variant="destructive" onClick={() => setForceStopOpen(true)}>
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
      </div>

      <div className="flex flex-col gap-2 px-1">
        <div>
          <p className="truncate font-medium">{instance.name}</p>
          <p className="truncate text-sm text-muted-foreground">
            {instance.minecraftVersion ?? "Unknown version"} · {instance.loader}
          </p>
        </div>

        {instance.status === "running" && <ResourceUsageRow instance={instance} />}

      <div className="flex flex-col gap-1">
        <Select
          value={instance.javaInstallationId ?? NO_JAVA_VALUE}
          onValueChange={handleJavaChange}
        >
          <SelectTrigger className="w-full" size="sm">
            {/* Resolved here rather than left to the Select's own lookup:
                base-ui derives the trigger's text from an `items` registry
                passed to the root, which we don't provide - without this it
                falls back to stringifying the raw value, i.e. printing the
                installation's UUID. */}
            <SelectValue placeholder="No Java selected">
              {(value) => {
                if (!value || value === NO_JAVA_VALUE) return "No Java selected";
                const java = installations.find((j) => j.id === value);
                return java
                  ? `Java ${java.version} (${java.architecture})`
                  : "Unknown Java installation";
              }}
            </SelectValue>
          </SelectTrigger>
          {/* Wider than the trigger and no longer locked to its width
              (`alignItemWithTrigger`/`w-(--anchor-width)` from the shared
              component would otherwise clip long entries like "Java 8.0.503
              (x64) · Recommended" against the card's narrow trigger width -
              see the truncation below for the same problem's other half). */}
          <SelectContent alignItemWithTrigger={false} className="w-64">
            <SelectItem value={NO_JAVA_VALUE} label="No Java selected">
              No Java selected
            </SelectItem>
            {/* The assigned installation may no longer be in the detected
                list (e.g. uninstalled, or cleared via "Forget all Java
                installations"). Without this, the Select has no item to
                match the value against and falls back to showing the raw
                installation ID - a UUID - instead of a label. */}
            {instance.javaInstallationId && !assignedJava && (
              <SelectItem
                value={instance.javaInstallationId}
                label="Unknown Java installation"
                disabled
              >
                Unknown Java installation (not detected)
              </SelectItem>
            )}
            {installations.map((java) => {
              const major = parseJavaMajor(java.version);
              const recommended = requiredJava !== null && major === requiredJava;
              const label = `Java ${java.version} (${java.architecture})`;
              return (
                // `label` is required here, not cosmetic: the popup's items
                // are portaled and only exist in the DOM once opened, so
                // without an explicit label the trigger can only *guess*
                // the selected item's text from that (possibly-never-
                // mounted) DOM node - and falls back to showing the raw
                // installation ID (a UUID) when it can't.
                <SelectItem key={java.id} value={java.id} label={label} title={java.path}>
                  <span className="min-w-0 flex-1 truncate">{label}</span>
                  {recommended && (
                    <span className="shrink-0 rounded-full bg-primary/15 px-1.5 py-0.5 text-[10px] font-medium text-primary">
                      Recommended
                    </span>
                  )}
                </SelectItem>
              );
            })}
          </SelectContent>
        </Select>
        {javaMismatch && (
          <p className="text-xs text-destructive">
            Minecraft {instance.minecraftVersion} needs Java {requiredJava}; this instance is set
            to Java {assignedJavaMajor}.
          </p>
        )}
        {!assignedJava && requiredJava !== null && (
          <p className="text-xs text-muted-foreground">Recommended: Java {requiredJava}</p>
        )}
      </div>

      <div className="flex gap-2">
        {(instance.status === "stopped" || instance.status === "crashed") && (
          <Button
            size="sm"
            className="flex-1"
            disabled={isProcessActionPending || needsForgeInstall(instance)}
            title={
              needsForgeInstall(instance)
                ? "This Forge/NeoForge server hasn't been installed yet - see Manage"
                : undefined
            }
            onClick={() =>
              runProcessAction(() => api.startInstance(instance.id), "Failed to start instance")
            }
          >
            <Play />
            Start
          </Button>
        )}
        {instance.status === "starting" && (
          <Button
            variant={isStuckStarting ? "destructive" : "outline"}
            size="sm"
            className="flex-1"
            disabled={isProcessActionPending}
            title={
              isStuckStarting
                ? "No output for several minutes - this instance may be stuck"
                : "Stop this instance while it's still starting - e.g. if this was the wrong server"
            }
            onClick={() =>
              runProcessAction(() => api.forceStopInstance(instance.id), "Failed to stop instance")
            }
          >
            <OctagonX />
            {isProcessActionPending ? "Starting…" : isStuckStarting ? "Possibly Stuck - Stop" : "Cancel Start"}
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
            onClick={() => navigate(`/instances/${instance.id}/console`)}
          >
            Console
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="flex-1"
            onClick={() => navigate(`/instances/${instance.id}`)}
          >
            Manage
          </Button>
        </div>
      </div>

      <Dialog
        open={renameOpen}
        onOpenChange={(next) => {
          if (!next && isSubmitting) return;
          setRenameOpen(next);
        }}
      >
        <DialogContent showCloseButton={!isSubmitting}>
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

      <Dialog
        open={duplicateOpen}
        onOpenChange={(next) => {
          // Duplicating copies the entire server directory (mods, world,
          // config) - for a large modpack this can take a real amount of
          // time, so closing this mid-copy is the same class of bug as the
          // import dialog: the copy keeps running, but it looks abandoned.
          if (!next && isSubmitting) return;
          setDuplicateOpen(next);
        }}
      >
        <DialogContent showCloseButton={!isSubmitting}>
          <form onSubmit={handleDuplicate} className="flex flex-col gap-4">
            <DialogHeader>
              <DialogTitle>Duplicate instance</DialogTitle>
              <DialogDescription>
                Copies this instance's server files (mods, config, world) and
                settings into a new instance. Logs and backups aren't
                carried over.
              </DialogDescription>
            </DialogHeader>
            <Input
              autoFocus
              value={duplicateNameDraft}
              onChange={(e) => setDuplicateNameDraft(e.target.value)}
            />
            <DialogFooter>
              <Button type="submit" disabled={isSubmitting}>
                {isSubmitting ? "Duplicating…" : "Duplicate"}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>

      <AlertDialog
        open={deleteOpen}
        onOpenChange={(next) => {
          if (!next && isSubmitting) return;
          setDeleteOpen(next);
        }}
      >
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

      <AlertDialog
        open={forceStopOpen}
        onOpenChange={(next) => {
          if (!next && isProcessActionPending) return;
          setForceStopOpen(next);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Force stop "{instance.name}"?</AlertDialogTitle>
            <AlertDialogDescription>
              This kills the server process immediately without letting it
              save. Any unsaved world changes since the last autosave will
              be lost. Use "Stop" instead if the server is responsive.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              disabled={isProcessActionPending}
              onClick={handleForceStop}
            >
              {isProcessActionPending ? "Forcing…" : "Force Stop"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </li>
  );
}
