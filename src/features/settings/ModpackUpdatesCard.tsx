import { useEffect, useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Download, Link2, List, RefreshCw, Search, Unlink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
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
import { api } from "@/lib/tauri";
import type { Instance } from "@/types/instance";
import type { ModpackUpdateCheck, ModrinthSearchHit, ModrinthVersion } from "@/types/modrinth";

function LinkProjectDialog({
  instance,
  open,
  onOpenChange,
}: {
  instance: Instance;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { linkModrinthProject } = useInstancesStore();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ModrinthSearchHit[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [linkingId, setLinkingId] = useState<string | null>(null);

  useEffect(() => {
    if (!query.trim()) {
      setResults([]);
      return;
    }
    setIsSearching(true);
    const timeout = setTimeout(() => {
      api
        .searchModrinthProjects(query.trim())
        .then(setResults)
        .catch((err) => toast.error("Search failed", { description: String(err) }))
        .finally(() => setIsSearching(false));
    }, 350);
    return () => clearTimeout(timeout);
  }, [query]);

  async function handleLink(hit: ModrinthSearchHit) {
    setLinkingId(hit.projectId);
    try {
      await linkModrinthProject(instance.id, hit.projectId);
      toast.success(`Linked to "${hit.title}"`);
      onOpenChange(false);
    } catch (err) {
      toast.error("Failed to link project", { description: String(err) });
    } finally {
      setLinkingId(null);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && linkingId) return;
        onOpenChange(next);
        if (!next) {
          setQuery("");
          setResults([]);
        }
      }}
    >
      <DialogContent showCloseButton={!linkingId}>
        <DialogHeader>
          <DialogTitle>Link a Modrinth project</DialogTitle>
          <DialogDescription>
            Search for the modpack this instance was built from. Once linked, you can check
            for and install newer versions from here. If the pack isn't on Modrinth at all,
            there's nothing to link to - updates would have to stay manual.
          </DialogDescription>
        </DialogHeader>
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            autoFocus
            className="pl-8"
            placeholder="Search modpacks…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </div>
        <div className="flex max-h-80 flex-col gap-1 overflow-y-auto">
          {isSearching && (
            <p className="p-2 text-sm text-muted-foreground">Searching…</p>
          )}
          {!isSearching && query.trim() && results.length === 0 && (
            <p className="p-2 text-sm text-muted-foreground">No modpacks found.</p>
          )}
          {results.map((hit) => (
            <button
              key={hit.projectId}
              type="button"
              disabled={linkingId !== null}
              onClick={() => handleLink(hit)}
              className="flex items-center gap-3 rounded-lg p-2 text-left transition-colors hover:bg-muted disabled:opacity-50"
            >
              {hit.iconUrl ? (
                <img src={hit.iconUrl} alt="" className="size-9 shrink-0 rounded-md object-cover" />
              ) : (
                <div className="size-9 shrink-0 rounded-md bg-muted" />
              )}
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium">{hit.title}</p>
                <p className="truncate text-xs text-muted-foreground">{hit.description}</p>
              </div>
              {linkingId === hit.projectId && (
                <span className="shrink-0 text-xs text-muted-foreground">Linking…</span>
              )}
            </button>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}

/** A version is flagged, not blocked, when it doesn't match - the operator
 * might know better than the auto-picker (e.g. testing a migration). */
function versionMismatches(instance: Instance, version: ModrinthVersion): boolean {
  const loaderOk =
    instance.loader === "unknown" ||
    version.loaders.some((l) => l.toLowerCase() === instance.loader);
  const mcOk =
    !instance.minecraftVersion || version.gameVersions.includes(instance.minecraftVersion);
  return !loaderOk || !mcOk;
}

function BrowseVersionsDialog({
  instance,
  open,
  onOpenChange,
  onInstalled,
}: {
  instance: Instance;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onInstalled: (version: ModrinthVersion) => void;
}) {
  const { applyModpackUpdate } = useInstancesStore();
  const [versions, setVersions] = useState<ModrinthVersion[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [installingId, setInstallingId] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setIsLoading(true);
    api
      .listModpackVersions(instance.id)
      .then(setVersions)
      .catch((err) => toast.error("Failed to load versions", { description: String(err) }))
      .finally(() => setIsLoading(false));
  }, [open, instance.id]);

  async function handleInstall(version: ModrinthVersion) {
    setInstallingId(version.id);
    try {
      await applyModpackUpdate(instance.id, version.id);
      toast.success(`Installed ${version.versionNumber}`);
      onInstalled(version);
      onOpenChange(false);
    } catch (err) {
      toast.error("Failed to install version", { description: String(err) });
    } finally {
      setInstallingId(null);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && installingId) return;
        onOpenChange(next);
      }}
    >
      <DialogContent showCloseButton={!installingId}>
        <DialogHeader>
          <DialogTitle>Browse versions</DialogTitle>
          <DialogDescription>
            Every published version of "{instance.modrinthProjectTitle}", newest first. Versions
            that don't match this instance's loader or Minecraft version are flagged, but you
            can still install one if you know what you're doing.
          </DialogDescription>
        </DialogHeader>
        <div className="flex max-h-96 flex-col gap-1 overflow-y-auto">
          {isLoading && <p className="p-2 text-sm text-muted-foreground">Loading…</p>}
          {!isLoading && versions.length === 0 && (
            <p className="p-2 text-sm text-muted-foreground">No published versions found.</p>
          )}
          {versions.map((version) => {
            const mismatch = versionMismatches(instance, version);
            const isCurrent = version.id === instance.modrinthVersionId;
            return (
              <div
                key={version.id}
                className="flex items-center justify-between gap-3 rounded-lg border border-border p-2.5"
              >
                <div className="min-w-0">
                  <p className="flex items-center gap-1.5 truncate text-sm font-medium">
                    {version.name} ({version.versionNumber})
                    {isCurrent && <Badge variant="secondary">Installed</Badge>}
                    {mismatch && (
                      <span
                        className="flex items-center gap-1 text-xs text-amber-600 dark:text-amber-400"
                        title="Doesn't match this instance's loader/Minecraft version"
                      >
                        <AlertTriangle className="size-3" />
                        Mismatch
                      </span>
                    )}
                  </p>
                  <p className="truncate text-xs text-muted-foreground">
                    {version.loaders.join(", ") || "Unknown loader"} ·{" "}
                    {version.gameVersions.join(", ") || "Unknown MC version"}
                  </p>
                </div>
                <Button
                  size="sm"
                  variant={isCurrent ? "outline" : "default"}
                  disabled={installingId !== null}
                  onClick={() => handleInstall(version)}
                  className="shrink-0"
                >
                  {installingId === version.id
                    ? "Installing…"
                    : isCurrent
                      ? "Reinstall"
                      : "Install"}
                </Button>
              </div>
            );
          })}
        </div>
      </DialogContent>
    </Dialog>
  );
}

export function ModpackUpdatesCard({ instance }: { instance: Instance }) {
  const { unlinkModrinthProject, applyModpackUpdate } = useInstancesStore();
  const [linkDialogOpen, setLinkDialogOpen] = useState(false);
  const [browseOpen, setBrowseOpen] = useState(false);
  const [unlinkOpen, setUnlinkOpen] = useState(false);
  const [checkResult, setCheckResult] = useState<ModpackUpdateCheck | null>(null);
  const [isChecking, setIsChecking] = useState(false);
  const [isApplying, setIsApplying] = useState(false);

  const isLinked = instance.modrinthProjectId !== null;

  async function handleCheck() {
    setIsChecking(true);
    setCheckResult(null);
    try {
      setCheckResult(await api.checkModpackUpdate(instance.id));
    } catch (err) {
      toast.error("Failed to check for updates", { description: String(err) });
    } finally {
      setIsChecking(false);
    }
  }

  async function handleUnlink() {
    try {
      await unlinkModrinthProject(instance.id);
      setCheckResult(null);
      setUnlinkOpen(false);
      toast.success("Modrinth project unlinked");
    } catch (err) {
      toast.error("Failed to unlink project", { description: String(err) });
    }
  }

  async function handleUpdate() {
    const version = checkResult?.latestVersion;
    if (!version) return;
    setIsApplying(true);
    try {
      await applyModpackUpdate(instance.id, version.id);
      toast.success(`Updated to ${version.versionNumber}`);
      setCheckResult((prev) =>
        prev ? { ...prev, hasUpdate: false, currentVersionId: version.id } : prev,
      );
    } catch (err) {
      toast.error("Failed to install update", { description: String(err) });
    } finally {
      setIsApplying(false);
    }
  }

  return (
    <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h2 className="text-sm font-medium">Modpack Updates</h2>
          <p className="text-xs text-muted-foreground">
            {isLinked
              ? `Linked to "${instance.modrinthProjectTitle}" on Modrinth.`
              : "Link this instance to a Modrinth project to check for and install updates."}
          </p>
        </div>
        {isLinked ? (
          <Button variant="outline" size="sm" onClick={() => setUnlinkOpen(true)}>
            <Unlink />
            Unlink
          </Button>
        ) : (
          <Button variant="outline" size="sm" onClick={() => setLinkDialogOpen(true)}>
            <Link2 />
            Link Project
          </Button>
        )}
      </div>

      {isLinked && (
        <div className="flex flex-col gap-3">
          <div className="flex gap-2">
            <Button variant="outline" size="sm" className="w-fit" disabled={isChecking} onClick={handleCheck}>
              <RefreshCw className={isChecking ? "animate-spin" : ""} />
              {isChecking ? "Checking…" : "Check for Updates"}
            </Button>
            <Button variant="ghost" size="sm" className="w-fit" onClick={() => setBrowseOpen(true)}>
              <List />
              Browse Versions
            </Button>
          </div>

          {checkResult && !checkResult.hasUpdate && checkResult.latestVersion && (
            <p className="text-sm text-muted-foreground">Up to date.</p>
          )}

          {checkResult && !checkResult.latestVersion && (
            <p className="flex items-center gap-1.5 text-sm text-amber-600 dark:text-amber-400">
              <AlertTriangle className="size-4 shrink-0" />
              No published version matches this instance's loader/Minecraft version. Try
              "Browse Versions" to pick one manually.
            </p>
          )}

          {checkResult?.hasUpdate && checkResult.latestVersion && (
            <div className="flex flex-col gap-2 rounded-lg border border-primary/30 bg-primary/5 p-3">
              <div>
                <p className="text-sm font-medium">
                  {checkResult.latestVersion.name} ({checkResult.latestVersion.versionNumber})
                </p>
                <p className="text-xs text-muted-foreground">
                  Published {new Date(checkResult.latestVersion.datePublished).toLocaleDateString()}
                </p>
              </div>
              {checkResult.latestVersion.changelog && (
                <p className="max-h-32 overflow-y-auto whitespace-pre-wrap text-xs text-muted-foreground">
                  {checkResult.latestVersion.changelog}
                </p>
              )}
              <Button size="sm" className="w-fit" disabled={isApplying} onClick={handleUpdate}>
                <Download />
                {isApplying ? "Installing… please don't close ModpackPilot" : "Update Now"}
              </Button>
            </div>
          )}
        </div>
      )}

      <LinkProjectDialog instance={instance} open={linkDialogOpen} onOpenChange={setLinkDialogOpen} />

      <BrowseVersionsDialog
        instance={instance}
        open={browseOpen}
        onOpenChange={setBrowseOpen}
        onInstalled={(version) =>
          setCheckResult({ hasUpdate: false, currentVersionId: version.id, latestVersion: version })
        }
      />

      <AlertDialog open={unlinkOpen} onOpenChange={setUnlinkOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Unlink Modrinth project?</AlertDialogTitle>
            <AlertDialogDescription>
              This only removes the update-check link - nothing on disk changes, and you can
              re-link at any time.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction onClick={handleUnlink}>Unlink</AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </section>
  );
}
