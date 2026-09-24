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
import { api, listenWithCleanup } from "@/lib/tauri";
import type { Instance } from "@/types/instance";
import {
  FTB_INSTALL_PROGRESS_EVENT,
  type FtbInstallProgress,
  type FtbPack,
  type FtbUpdateCheck,
  type FtbVersionSummary,
} from "@/types/ftb";
import { UpdatePolicySelect } from "./UpdatePolicySelect";

function LinkFtbPackDialog({
  instance,
  open,
  onOpenChange,
}: {
  instance: Instance;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const { linkFtbPack } = useInstancesStore();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<FtbPack[]>([]);
  const [isSearching, setIsSearching] = useState(false);
  const [linkingId, setLinkingId] = useState<number | null>(null);

  useEffect(() => {
    if (!query.trim()) {
      setResults([]);
      return;
    }
    setIsSearching(true);
    const timeout = setTimeout(() => {
      api
        .searchFtbPacks(query.trim())
        .then(setResults)
        .catch((err) => toast.error("Search failed", { description: String(err) }))
        .finally(() => setIsSearching(false));
    }, 350);
    return () => clearTimeout(timeout);
  }, [query]);

  async function handleLink(pack: FtbPack) {
    setLinkingId(pack.id);
    try {
      await linkFtbPack(instance.id, pack.id);
      toast.success(`Linked to "${pack.name}"`);
      onOpenChange(false);
    } catch (err) {
      toast.error("Failed to link modpack", { description: String(err) });
    } finally {
      setLinkingId(null);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && linkingId !== null) return;
        onOpenChange(next);
        if (!next) {
          setQuery("");
          setResults([]);
        }
      }}
    >
      <DialogContent showCloseButton={linkingId === null}>
        <DialogHeader>
          <DialogTitle>Link an FTB modpack</DialogTitle>
          <DialogDescription>
            Search Feed the Beast for the modpack this instance runs. Once linked, you can
            check for and install newer versions from here - including packs first set up
            with FTB's own server installer.
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
          {isSearching && <p className="p-2 text-sm text-muted-foreground">Searching…</p>}
          {!isSearching && query.trim() && results.length === 0 && (
            <p className="p-2 text-sm text-muted-foreground">No modpacks found.</p>
          )}
          {results.map((pack) => (
            <button
              key={pack.id}
              type="button"
              disabled={linkingId !== null}
              onClick={() => handleLink(pack)}
              className="flex items-center gap-3 rounded-lg p-2 text-left transition-colors hover:bg-muted disabled:opacity-50"
            >
              {pack.iconUrl ? (
                <img src={pack.iconUrl} alt="" className="size-9 shrink-0 rounded-md object-cover" />
              ) : (
                <div className="size-9 shrink-0 rounded-md bg-muted" />
              )}
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium">{pack.name}</p>
                <p className="truncate text-xs text-muted-foreground">{pack.synopsis}</p>
              </div>
              {linkingId === pack.id && (
                <span className="shrink-0 text-xs text-muted-foreground">Linking…</span>
              )}
            </button>
          ))}
        </div>
      </DialogContent>
    </Dialog>
  );
}

/** Flagged, not blocked - the operator may know better than the auto-picker
 * (testing a migration, say). */
function versionMismatches(instance: Instance, version: FtbVersionSummary): boolean {
  const loaderTarget = version.targets.find((t) => t.type === "modloader");
  const mcTarget = version.targets.find((t) => t.name === "minecraft");
  const loaderOk =
    instance.loader === "unknown" ||
    !loaderTarget ||
    loaderTarget.name.toLowerCase() === instance.loader;
  const mcOk = !instance.minecraftVersion || !mcTarget || mcTarget.version === instance.minecraftVersion;
  return !loaderOk || !mcOk;
}

function describeTargets(version: FtbVersionSummary): string {
  const mc = version.targets.find((t) => t.name === "minecraft");
  const loader = version.targets.find((t) => t.type === "modloader");
  return [
    mc ? `MC ${mc.version}` : "Unknown MC version",
    loader ? `${loader.name} ${loader.version}` : "Vanilla",
  ].join(" · ");
}

function BrowseFtbVersionsDialog({
  instance,
  open,
  onOpenChange,
  onInstalled,
  isRunning,
}: {
  instance: Instance;
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onInstalled: (version: FtbVersionSummary) => void;
  isRunning: boolean;
}) {
  const { applyFtbUpdate } = useInstancesStore();
  const [versions, setVersions] = useState<FtbVersionSummary[]>([]);
  const [isLoading, setIsLoading] = useState(false);
  const [installingId, setInstallingId] = useState<number | null>(null);

  useEffect(() => {
    if (!open) return;
    setIsLoading(true);
    api
      .listFtbVersions(instance.id)
      .then(setVersions)
      .catch((err) => toast.error("Failed to load versions", { description: String(err) }))
      .finally(() => setIsLoading(false));
  }, [open, instance.id]);

  async function handleInstall(version: FtbVersionSummary) {
    setInstallingId(version.id);
    try {
      await applyFtbUpdate(instance.id, version.id);
      toast.success(`Installed ${version.name}`);
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
        if (!next && installingId !== null) return;
        onOpenChange(next);
      }}
    >
      <DialogContent showCloseButton={installingId === null}>
        <DialogHeader>
          <DialogTitle>Browse versions</DialogTitle>
          <DialogDescription>
            Every published version of "{instance.ftbPackName}", newest first. Versions that
            don't match this instance's loader or Minecraft version are flagged, but you can
            still install one if you know what you're doing.
          </DialogDescription>
        </DialogHeader>
        <div className="flex max-h-96 flex-col gap-1 overflow-y-auto">
          {isLoading && <p className="p-2 text-sm text-muted-foreground">Loading…</p>}
          {!isLoading && versions.length === 0 && (
            <p className="p-2 text-sm text-muted-foreground">No published versions found.</p>
          )}
          {versions.map((version) => {
            const mismatch = versionMismatches(instance, version);
            const isCurrent = version.id === instance.ftbVersionId;
            return (
              <div
                key={version.id}
                className="flex items-center justify-between gap-3 rounded-lg border border-border p-2.5"
              >
                <div className="min-w-0">
                  <p className="flex items-center gap-1.5 truncate text-sm font-medium">
                    {version.name}
                    {version.type !== "release" && (
                      <Badge variant="outline">{version.type}</Badge>
                    )}
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
                    {describeTargets(version)}
                  </p>
                </div>
                <Button
                  size="sm"
                  variant={isCurrent ? "outline" : "default"}
                  disabled={installingId !== null || isRunning}
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
        {isRunning && (
          <p className="text-xs text-muted-foreground">
            Stop the server first - swapping mod files under a running server corrupts it.
          </p>
        )}
      </DialogContent>
    </Dialog>
  );
}

/** The FTB half of the Modpack Updates card. Renders just a "link" button
 * when nothing is linked yet, and the full check/browse/install controls
 * once it is. */
export function FtbUpdatesSection({ instance }: { instance: Instance }) {
  const { unlinkFtbPack, applyFtbUpdate, setUpdatePolicy } = useInstancesStore();
  const [linkOpen, setLinkOpen] = useState(false);
  const [browseOpen, setBrowseOpen] = useState(false);
  const [unlinkOpen, setUnlinkOpen] = useState(false);
  const [checkResult, setCheckResult] = useState<FtbUpdateCheck | null>(null);
  const [isChecking, setIsChecking] = useState(false);
  const [isApplying, setIsApplying] = useState(false);
  const [progress, setProgress] = useState<FtbInstallProgress | null>(null);

  const isLinked = instance.ftbPackId !== null;
  const isRunning = instance.status !== "stopped" && instance.status !== "crashed";

  useEffect(() => {
    if (!isApplying) return;
    return listenWithCleanup<FtbInstallProgress>(FTB_INSTALL_PROGRESS_EVENT, (event) => {
      setProgress(event.payload);
    });
  }, [isApplying]);

  async function handleCheck() {
    setIsChecking(true);
    setCheckResult(null);
    try {
      setCheckResult(await api.checkFtbUpdate(instance.id));
    } catch (err) {
      toast.error("Failed to check for updates", { description: String(err) });
    } finally {
      setIsChecking(false);
    }
  }

  async function handleUnlink() {
    try {
      await unlinkFtbPack(instance.id);
      setCheckResult(null);
      setUnlinkOpen(false);
      toast.success("FTB modpack unlinked");
    } catch (err) {
      toast.error("Failed to unlink modpack", { description: String(err) });
    }
  }

  async function handleUpdate() {
    const version = checkResult?.latestVersion;
    if (!version) return;
    setIsApplying(true);
    setProgress(null);
    try {
      await applyFtbUpdate(instance.id, version.id);
      toast.success(`Updated to ${version.name}`);
      setCheckResult((prev) =>
        prev ? { ...prev, hasUpdate: false, currentVersionId: version.id } : prev,
      );
    } catch (err) {
      toast.error("Failed to install update", { description: String(err) });
    } finally {
      setIsApplying(false);
    }
  }

  if (!isLinked) {
    return (
      <>
        <Button variant="outline" size="sm" onClick={() => setLinkOpen(true)}>
          <Link2 />
          Link FTB Pack
        </Button>
        <LinkFtbPackDialog instance={instance} open={linkOpen} onOpenChange={setLinkOpen} />
      </>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between gap-2">
        <div>
          <h2 className="text-sm font-medium">Modpack Updates</h2>
          <p className="text-xs text-muted-foreground">
            Linked to "{instance.ftbPackName}" on FTB.
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={() => setUnlinkOpen(true)}>
          <Unlink />
          Unlink
        </Button>
      </div>

      <div className="flex gap-2">
        <Button
          variant="outline"
          size="sm"
          className="w-fit"
          disabled={isChecking}
          onClick={handleCheck}
        >
          <RefreshCw className={isChecking ? "animate-spin" : ""} />
          {isChecking ? "Checking…" : "Check for Updates"}
        </Button>
        <Button variant="ghost" size="sm" className="w-fit" onClick={() => setBrowseOpen(true)}>
          <List />
          Browse Versions
        </Button>
      </div>

      <UpdatePolicySelect
        value={instance.updatePolicy}
        onChange={(policy) => {
          setUpdatePolicy(instance.id, policy).catch((err) =>
            toast.error("Failed to change update policy", { description: String(err) }),
          );
        }}
      />

      {checkResult && !checkResult.hasUpdate && checkResult.latestVersion && (
        <p className="text-sm text-muted-foreground">Up to date.</p>
      )}

      {checkResult && !checkResult.latestVersion && (
        <p className="flex items-center gap-1.5 text-sm text-amber-600 dark:text-amber-400">
          <AlertTriangle className="size-4 shrink-0" />
          No published version matches this instance's loader/Minecraft version. Try "Browse
          Versions" to pick one manually.
        </p>
      )}

      {checkResult?.hasUpdate && checkResult.latestVersion && (
        <div className="flex flex-col gap-2 rounded-lg border border-primary/30 bg-primary/5 p-3">
          <div>
            <p className="text-sm font-medium">{checkResult.latestVersion.name}</p>
            <p className="text-xs text-muted-foreground">
              {describeTargets(checkResult.latestVersion)}
            </p>
          </div>

          {isApplying && (
            <div className="flex flex-col gap-1.5 text-xs text-muted-foreground">
              <p>
                {progress?.phase === "installing-loader"
                  ? (progress.detail ?? "Installing the mod loader…")
                  : `Downloading ${progress?.done ?? 0} of ${progress?.total ?? 0} files…`}
              </p>
              <div className="h-1.5 overflow-hidden rounded-full bg-border">
                <div
                  className="h-full bg-primary transition-[width]"
                  style={{
                    width: `${progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0}%`,
                  }}
                />
              </div>
            </div>
          )}

          <Button
            size="sm"
            className="w-fit"
            disabled={isApplying || isRunning}
            onClick={handleUpdate}
          >
            <Download />
            {isApplying ? "Installing… please don't close ModpackPilot" : "Update Now"}
          </Button>
          {isRunning && (
            <p className="text-xs text-muted-foreground">
              Stop the server first - swapping mod files under a running server corrupts it.
            </p>
          )}
        </div>
      )}

      <BrowseFtbVersionsDialog
        instance={instance}
        open={browseOpen}
        onOpenChange={setBrowseOpen}
        isRunning={isRunning}
        onInstalled={(version) =>
          setCheckResult({
            hasUpdate: false,
            currentVersionId: version.id,
            latestVersion: version,
          })
        }
      />

      <AlertDialog open={unlinkOpen} onOpenChange={setUnlinkOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Unlink FTB modpack?</AlertDialogTitle>
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
    </div>
  );
}
