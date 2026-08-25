import { useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Hammer } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useInstancesStore } from "@/stores/instancesStore";
import type { Instance } from "@/types/instance";

/** True when this instance's `server_jar` is (or looks like) a Forge/
 * NeoForge installer rather than an actual runnable server - either it was
 * imported before the installer-exclusion fix, or it's a fresh pack that
 * genuinely hasn't been installed yet. Either way, starting it as-is would
 * just pop up the installer's own GUI instead of a server. */
export function needsForgeInstall(instance: Instance): boolean {
  if (instance.loader !== "forge" && instance.loader !== "neoforge") return false;
  return !instance.serverJar || instance.serverJar.toLowerCase().includes("installer");
}

export function ForgeInstallBanner({ instance }: { instance: Instance }) {
  const { installForgeServer } = useInstancesStore();
  const [isInstalling, setIsInstalling] = useState(false);

  if (!needsForgeInstall(instance)) return null;

  async function handleInstall() {
    setIsInstalling(true);
    try {
      await installForgeServer(instance.id);
      toast.success("Server installed - it's ready to start.");
    } catch (err) {
      toast.error("Installation failed", { description: String(err) });
    } finally {
      setIsInstalling(false);
    }
  }

  return (
    <div className="flex flex-col gap-2 rounded-xl border border-amber-500/30 bg-amber-500/5 p-4">
      <p className="flex items-center gap-1.5 text-sm font-medium text-amber-600 dark:text-amber-400">
        <AlertTriangle className="size-4 shrink-0" />
        This server isn't installed yet
      </p>
      <p className="text-sm text-muted-foreground">
        Modern Forge/NeoForge packs ship an installer, not a ready-to-run server. Starting this
        instance as-is just opens that installer's window instead of booting a server. Run it
        once here and this only needs doing this one time.
      </p>
      <Button size="sm" className="w-fit" disabled={isInstalling} onClick={handleInstall}>
        <Hammer />
        {isInstalling ? "Installing… this can take a minute" : "Install Forge/NeoForge Server"}
      </Button>
    </div>
  );
}
