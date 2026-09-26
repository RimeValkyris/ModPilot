import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Archive, Folder, RotateCcw, ShieldCheck, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
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
import { formatFileSize } from "@/lib/format";
import type { Instance } from "@/types/instance";
import type { WorldBackup } from "@/types/backup";

const FOLDERS = [
  { key: "server", label: "Server" },
  { key: "mods", label: "Mods" },
  { key: "config", label: "Config" },
  { key: "world", label: "World" },
] as const;

export function InstanceFilesTab({ instance }: { instance: Instance }) {
  const [backups, setBackups] = useState<WorldBackup[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isCreating, setIsCreating] = useState(false);
  const [pendingRestore, setPendingRestore] = useState<string | null>(null);
  const [pendingDelete, setPendingDelete] = useState<string | null>(null);
  const [isRestoring, setIsRestoring] = useState(false);
  const [isDeletingBackup, setIsDeletingBackup] = useState(false);
  /** Which backup is currently being read end to end, so only that row's
   * button shows a pending state rather than all of them. */
  const [verifying, setVerifying] = useState<string | null>(null);
  /** Worlds that previous restores moved aside. Full copies of a modded
   * world, so leaving them invisible would quietly consume the disk. */
  const [preRestore, setPreRestore] = useState<WorldBackup[]>([]);
  const [pendingPreRestoreDelete, setPendingPreRestoreDelete] = useState<string | null>(null);
  const [isDeletingPreRestore, setIsDeletingPreRestore] = useState(false);

  async function refresh() {
    setIsLoading(true);
    try {
      const [saved, displaced] = await Promise.all([
        api.listWorldBackups(instance.id),
        api.listPreRestoreWorlds(instance.id),
      ]);
      setBackups(saved);
      setPreRestore(displaced);
    } catch (err) {
      toast.error("Failed to load backups", { description: String(err) });
    } finally {
      setIsLoading(false);
    }
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instance.id]);

  async function handleOpenFolder(folder: (typeof FOLDERS)[number]["key"]) {
    try {
      await api.openManagedFolder({ instanceSubfolder: { id: instance.id, folder } });
    } catch (err) {
      toast.error(`Failed to open ${folder} folder`, { description: String(err) });
    }
  }

  async function handleCreateBackup() {
    setIsCreating(true);
    try {
      await api.createWorldBackup(instance.id);
      toast.success("Backup created");
      await refresh();
    } catch (err) {
      toast.error("Failed to create backup", { description: String(err) });
    } finally {
      setIsCreating(false);
    }
  }

  async function handleVerify(name: string) {
    setVerifying(name);
    try {
      const result = await api.verifyWorldBackup(instance.id, name);
      toast.success("Backup verified", {
        description: `${result.fileCount} files, ${formatFileSize(result.uncompressedBytes)} uncompressed.`,
      });
    } catch (err) {
      toast.error("This backup cannot be restored", { description: String(err) });
    } finally {
      setVerifying(null);
    }
  }

  async function handleRestore(name: string) {
    setIsRestoring(true);
    try {
      const result = await api.restoreWorldBackup(instance.id, name);
      // The old world is moved aside, never deleted - saying so is the
      // whole point, otherwise nobody knows the folder is there to reclaim.
      toast.success("World restored", {
        description: result.displacedWorld
          ? `${result.fileCount} files restored. The previous world was kept as "${result.displacedWorld}" - delete it yourself once you're happy.`
          : `${result.fileCount} files restored.`,
        duration: 10000,
      });
      setPendingRestore(null);
      await refresh();
    } catch (err) {
      toast.error("Failed to restore backup", { description: String(err) });
    } finally {
      setIsRestoring(false);
    }
  }

  async function handleDeletePreRestore(name: string) {
    setIsDeletingPreRestore(true);
    try {
      await api.deletePreRestoreWorld(instance.id, name);
      setPreRestore((prev) => prev.filter((w) => w.name !== name));
      setPendingPreRestoreDelete(null);
      toast.success("Saved world deleted");
    } catch (err) {
      toast.error("Failed to delete saved world", { description: String(err) });
    } finally {
      setIsDeletingPreRestore(false);
    }
  }

  async function handleDelete(name: string) {
    setIsDeletingBackup(true);
    try {
      await api.deleteWorldBackup(instance.id, name);
      setBackups((prev) => prev.filter((b) => b.name !== name));
      setPendingDelete(null);
    } catch (err) {
      toast.error("Failed to delete backup", { description: String(err) });
    } finally {
      setIsDeletingBackup(false);
    }
  }

  const canRestore = instance.status === "stopped" || instance.status === "crashed";

  return (
    <div className="flex flex-col gap-4">
      <section className="rounded-xl border border-border bg-card p-4">
        <h2 className="mb-3 text-sm font-medium">Folders</h2>
        <div className="flex flex-wrap gap-2">
          {FOLDERS.map(({ key, label }) => (
            <Button key={key} variant="outline" size="sm" onClick={() => handleOpenFolder(key)}>
              <Folder />
              {label}
            </Button>
          ))}
        </div>
      </section>

      <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="text-sm font-medium">World Backups</h2>
            <p className="text-xs text-muted-foreground">
              Snapshots of the world folder, stored in this instance's own backups/ folder.
            </p>
          </div>
          <Button size="sm" disabled={isCreating} onClick={handleCreateBackup}>
            <Archive />
            {isCreating ? "Creating…" : "Create Backup"}
          </Button>
        </div>

        {isLoading ? (
          <p className="text-sm text-muted-foreground">Loading…</p>
        ) : backups.length === 0 ? (
          <p className="text-sm text-muted-foreground">No backups yet.</p>
        ) : (
          <ul className="flex flex-col gap-2">
            {backups.map((backup) => (
              <li
                key={backup.name}
                className="flex items-center justify-between gap-3 rounded-lg border border-border p-2.5 text-sm"
              >
                <div className="min-w-0">
                  <p className="truncate font-medium" title={backup.name}>
                    {backup.name}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    {formatFileSize(backup.sizeBytes)} ·{" "}
                    {new Date(backup.createdAt).toLocaleString()}
                  </p>
                </div>
                <div className="flex shrink-0 gap-1.5">
                  <Button
                    variant="outline"
                    size="icon-sm"
                    title="Verify this backup can be restored"
                    disabled={verifying !== null}
                    onClick={() => handleVerify(backup.name)}
                  >
                    <ShieldCheck />
                  </Button>
                  <Button
                    variant="outline"
                    size="icon-sm"
                    title={canRestore ? "Restore" : "Stop the instance to restore"}
                    disabled={!canRestore}
                    onClick={() => setPendingRestore(backup.name)}
                  >
                    <RotateCcw />
                  </Button>
                  <Button
                    variant="outline"
                    size="icon-sm"
                    title="Delete this backup"
                    onClick={() => setPendingDelete(backup.name)}
                  >
                    <Trash2 />
                  </Button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      {preRestore.length > 0 && (
        <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
          <div>
            <h2 className="text-sm font-medium">Worlds Kept From Restores</h2>
            <p className="text-xs text-muted-foreground">
              Restoring a backup moves the world it replaces aside instead of
              deleting it. These are those copies — each one is a full world,
              so delete them once you're sure you don't need them.
            </p>
          </div>
          <ul className="flex flex-col gap-2">
            {preRestore.map((world) => (
              <li
                key={world.name}
                className="flex items-center justify-between gap-3 rounded-lg border border-border p-2.5 text-sm"
              >
                <div className="min-w-0">
                  <p className="truncate font-medium" title={world.name}>
                    {world.name}
                  </p>
                  <p className="text-xs text-muted-foreground">
                    Kept {new Date(world.createdAt).toLocaleString()}
                  </p>
                </div>
                <Button
                  variant="outline"
                  size="icon-sm"
                  title={canRestore ? "Delete this saved world" : "Stop the instance first"}
                  disabled={!canRestore}
                  onClick={() => setPendingPreRestoreDelete(world.name)}
                >
                  <Trash2 />
                </Button>
              </li>
            ))}
          </ul>
        </section>
      )}

      <AlertDialog
        open={pendingRestore !== null}
        onOpenChange={(next) => {
          if (!next && isRestoring) return;
          if (!next) setPendingRestore(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Restore "{pendingRestore}"?</AlertDialogTitle>
            <AlertDialogDescription>
              The backup is checked before anything is changed, and the
              instance's current world is moved aside to a
              "pre-restore" folder rather than deleted - so you can put it
              back if this isn't the save you meant. Nothing is removed
              until you delete that folder yourself.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isRestoring}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              disabled={isRestoring}
              onClick={() => pendingRestore && handleRestore(pendingRestore)}
            >
              {isRestoring ? "Restoring…" : "Restore"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>

      <AlertDialog
        open={pendingDelete !== null}
        onOpenChange={(next) => {
          if (!next && isDeletingBackup) return;
          if (!next) setPendingDelete(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete "{pendingDelete}"?</AlertDialogTitle>
            <AlertDialogDescription>
              This permanently deletes the backup file. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isDeletingBackup}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              disabled={isDeletingBackup}
              onClick={() => pendingDelete && handleDelete(pendingDelete)}
            >
              {isDeletingBackup ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
      <AlertDialog
        open={pendingPreRestoreDelete !== null}
        onOpenChange={(next) => {
          if (!next && isDeletingPreRestore) return;
          if (!next) setPendingPreRestoreDelete(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete "{pendingPreRestoreDelete}"?</AlertDialogTitle>
            <AlertDialogDescription>
              This is a complete world that a previous restore set aside, and
              deleting it is permanent. It is not the world this instance
              currently uses — that one is untouched.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isDeletingPreRestore}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              disabled={isDeletingPreRestore}
              onClick={() =>
                pendingPreRestoreDelete && handleDeletePreRestore(pendingPreRestoreDelete)
              }
            >
              {isDeletingPreRestore ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
