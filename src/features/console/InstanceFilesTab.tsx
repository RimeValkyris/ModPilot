import { useEffect, useState } from "react";
import { toast } from "sonner";
import { openPath } from "@tauri-apps/plugin-opener";
import { Archive, Folder, RotateCcw, Trash2 } from "lucide-react";
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
import { formatMemoryMb } from "@/lib/format";
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

  async function refresh() {
    setIsLoading(true);
    try {
      setBackups(await api.listWorldBackups(instance.id));
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
      const path = await api.getInstanceSubfolder(instance.id, folder);
      await openPath(path);
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

  async function handleRestore(name: string) {
    setIsRestoring(true);
    try {
      await api.restoreWorldBackup(instance.id, name);
      toast.success("World restored");
      setPendingRestore(null);
    } catch (err) {
      toast.error("Failed to restore backup", { description: String(err) });
    } finally {
      setIsRestoring(false);
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
                <div>
                  <p className="font-medium">{backup.name}</p>
                  <p className="text-xs text-muted-foreground">
                    {formatMemoryMb(backup.sizeBytes / (1024 * 1024))} ·{" "}
                    {new Date(backup.createdAt).toLocaleString()}
                  </p>
                </div>
                <div className="flex gap-1.5">
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
              This replaces the instance's current world with this backup.
              The current world will be permanently overwritten. This cannot
              be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isRestoring}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
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
    </div>
  );
}
