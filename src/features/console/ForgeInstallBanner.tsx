import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Hammer } from "lucide-react";
import { Button } from "@/components/ui/button";
import { listenWithCleanup } from "@/lib/tauri";
import { useInstancesStore } from "@/stores/instancesStore";
import {
  LOADER_INSTALL_PROGRESS_EVENT,
  type LoaderInstallProgressPayload,
} from "@/types/events";
import { isBuildToolsLoader, isDirectDownloadLoader, type Instance } from "@/types/instance";

/** True when this instance's `server_jar` is (or looks like) a Forge/
 * NeoForge installer rather than an actual runnable server - either it was
 * imported before the installer-exclusion fix, or it's a fresh pack that
 * genuinely hasn't been installed yet. Either way, starting it as-is would
 * just pop up the installer's own GUI instead of a server. */
export function needsForgeInstall(instance: Instance): boolean {
  // Import detection says so outright for anything imported since it
  // learned to recognize this (see Rust's `DetectedServerInfo`).
  if (instance.launchMode === "installer") return true;
  // Older instances predate that flag, so they're still judged by what
  // their launch target looks like.
  if (instance.loader !== "forge" && instance.loader !== "neoforge") return false;
  return !instance.serverJar || instance.serverJar.toLowerCase().includes("installer");
}

/** A Vanilla/Paper/Purpur instance that has a Minecraft version but no
 * server jar yet - e.g. the download failed right after Create Server. */
export function needsPaperInstall(instance: Instance): boolean {
  return (
    isDirectDownloadLoader(instance.loader) &&
    !!instance.minecraftVersion &&
    (!instance.serverJar || instance.launchMode !== "jar")
  );
}

export function ForgeInstallBanner({ instance }: { instance: Instance }) {
  const { installForgeServer, installServerJar } = useInstancesStore();
  const [isInstalling, setIsInstalling] = useState(false);
  const [step, setStep] = useState<string | null>(null);

  // The banner unmounts the moment the install succeeds (the instance no
  // longer needs one), so late state updates have to be dropped rather
  // than applied to an unmounted component.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  useEffect(() => {
    if (!isInstalling) return;
    return listenWithCleanup<LoaderInstallProgressPayload>(LOADER_INSTALL_PROGRESS_EVENT, (event) => {
      if (event.payload.instanceId !== instance.id) return;
      if (mounted.current) setStep(event.payload.step);
    });
  }, [isInstalling, instance.id]);

  if (!needsForgeInstall(instance) && !needsPaperInstall(instance)) return null;

  async function handleInstall() {
    setIsInstalling(true);
    setStep(null);
    try {
      if (needsPaperInstall(instance)) {
        await installServerJar(instance.id);
      } else {
        await installForgeServer(instance.id);
      }
      toast.success("Server installed - it's ready to start.");
    } catch (err) {
      toast.error("Installation failed", { description: String(err) });
    } finally {
      if (mounted.current) {
        setIsInstalling(false);
        setStep(null);
      }
    }
  }

  return (
    <div className="flex flex-col gap-2 rounded-xl border border-amber-500/30 bg-amber-500/5 p-4">
      <p className="flex items-center gap-1.5 text-sm font-medium text-amber-600 dark:text-amber-400">
        <AlertTriangle className="size-4 shrink-0" />
        This server isn't installed yet
      </p>
      <p className="text-sm text-muted-foreground">
        {needsPaperInstall(instance)
          ? isBuildToolsLoader(instance.loader)
            ? "Spigot and Bukkit have no download - SpigotMC's BuildTools compiles them on this PC. It needs the right Java installed and an internet connection, and only has to be done once."
            : "This server's jar hasn't been downloaded yet. Download it once and the server is ready to start."
          : "Modern Forge/NeoForge packs ship an installer, not a ready-to-run server. Starting this instance as-is just opens that installer's window instead of booting a server. Run it once here and this only needs doing this one time."}
      </p>
      <Button size="sm" className="w-fit" disabled={isInstalling} onClick={handleInstall}>
        <Hammer />
        {isInstalling
          ? needsPaperInstall(instance) && isBuildToolsLoader(instance.loader)
            ? "Building…"
            : "Installing…"
          : needsPaperInstall(instance)
            ? isBuildToolsLoader(instance.loader)
              ? "Build Server"
              : "Download Server"
            : "Install Forge/NeoForge Server"}
      </Button>

      {isInstalling && (
        <div className="flex min-w-0 flex-col gap-1.5">
          {/* The installer never reports a percentage - it downloads
              Minecraft's libraries and then runs its own processors - so
              the bar is indeterminate and the line below it carries the
              actual information. */}
          <div
            className="h-1.5 overflow-hidden rounded-full bg-amber-500/15"
            role="progressbar"
            aria-label="Installing the loader server"
          >
            <div className="indeterminate-bar h-full rounded-full bg-amber-500" />
          </div>
          <p className="w-full truncate font-mono text-xs text-muted-foreground" title={step ?? undefined}>
            {step ?? "Starting the installer…"}
          </p>
          <p className="text-xs text-muted-foreground">
            {isBuildToolsLoader(instance.loader)
              ? "A build takes 5-15 minutes. Please don't close ModpackPilot."
              : "This takes a few minutes on a fresh pack. Please don't close ModpackPilot."}
          </p>
        </div>
      )}
    </div>
  );
}
