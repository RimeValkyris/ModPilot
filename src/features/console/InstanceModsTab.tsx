import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Switch } from "@/components/ui/switch";
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
import type { ModInfo } from "@/types/mod";

export function InstanceModsTab({ instance }: { instance: Instance }) {
  const [mods, setMods] = useState<ModInfo[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [pendingDelete, setPendingDelete] = useState<ModInfo | null>(null);
  const [isDeleting, setIsDeleting] = useState(false);

  async function refresh() {
    setIsLoading(true);
    try {
      setMods(await api.listMods(instance.id));
    } catch (err) {
      toast.error("Failed to load mods", { description: String(err) });
    } finally {
      setIsLoading(false);
    }
  }

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instance.id]);

  async function handleToggle(mod: ModInfo) {
    // Optimistic: flip immediately, fall back to a refresh if it fails.
    setMods((prev) =>
      prev.map((m) => (m.fileName === mod.fileName ? { ...m, enabled: !m.enabled } : m)),
    );
    try {
      await api.toggleMod(instance.id, mod.fileName);
      await refresh();
    } catch (err) {
      toast.error("Failed to toggle mod", { description: String(err) });
      await refresh();
    }
  }

  async function handleDelete(mod: ModInfo) {
    setIsDeleting(true);
    try {
      await api.deleteMod(instance.id, mod.fileName);
      setMods((prev) => prev.filter((m) => m.fileName !== mod.fileName));
      setPendingDelete(null);
    } catch (err) {
      toast.error("Failed to delete mod", { description: String(err) });
    } finally {
      setIsDeleting(false);
    }
  }

  const enabledCount = mods.filter((m) => m.enabled).length;

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <p className="text-sm text-muted-foreground">
          Disabling a mod renames it to <code>.jar.disabled</code> rather than
          deleting it, so it's easy to turn back on. Changes take effect the
          next time this instance starts.
        </p>
        {!isLoading && mods.length > 0 && (
          <Badge variant="secondary" className="shrink-0">
            {mods.length} mod{mods.length === 1 ? "" : "s"}
            {enabledCount !== mods.length ? ` · ${enabledCount} enabled` : ""}
          </Badge>
        )}
      </div>

      {isLoading ? (
        <p className="rounded-xl border border-border bg-card p-4 text-sm text-muted-foreground">
          Loading…
        </p>
      ) : mods.length === 0 ? (
        <p className="rounded-xl border border-dashed border-border bg-card p-4 text-sm text-muted-foreground">
          No mods found in this instance's mods folder.
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {mods.map((mod) => (
            <li
              key={mod.fileName}
              className="flex items-center justify-between gap-3 rounded-lg border border-border bg-card p-3 text-sm"
            >
              <div className="flex items-center gap-2 overflow-hidden">
                <Switch checked={mod.enabled} onCheckedChange={() => handleToggle(mod)} />
                <span className="truncate" title={mod.displayName}>
                  {mod.displayName}
                </span>
                {!mod.enabled && <Badge variant="outline">Disabled</Badge>}
              </div>
              <div className="flex shrink-0 items-center gap-3">
                <span className="text-xs text-muted-foreground">
                  {formatFileSize(mod.sizeBytes)}
                </span>
                <Button variant="ghost" size="icon-sm" onClick={() => setPendingDelete(mod)}>
                  <Trash2 />
                </Button>
              </div>
            </li>
          ))}
        </ul>
      )}

      <AlertDialog
        open={pendingDelete !== null}
        onOpenChange={(next) => {
          if (!next && isDeleting) return;
          if (!next) setPendingDelete(null);
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Delete "{pendingDelete?.displayName}"?</AlertDialogTitle>
            <AlertDialogDescription>
              This permanently deletes the mod file. This cannot be undone.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel disabled={isDeleting}>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              disabled={isDeleting}
              onClick={() => pendingDelete && handleDelete(pendingDelete)}
            >
              {isDeleting ? "Deleting…" : "Delete"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </div>
  );
}
