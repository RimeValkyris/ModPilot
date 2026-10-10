import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { CheckCircle2, Download, Loader2, TriangleAlert } from "lucide-react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/tauri";
import { getRequiredJavaMajor, parseJavaMajor } from "@/lib/javaRequirement";

/** Majors ModpackPilot's managed installer can fetch (Rust's
 * `java::install_temurin`). Anything else has to be installed by hand. */
const INSTALLABLE_MAJORS = [8, 17, 21, 25];

/**
 * Tells the operator whether the Java a Minecraft version needs is already
 * installed, and offers to install it when it isn't - instead of finding out
 * when the server refuses to start.
 */
export function JavaRequirementNotice({ minecraftVersion }: { minecraftVersion: string }) {
  const required = getRequiredJavaMajor(minecraftVersion);
  const [installedMajors, setInstalledMajors] = useState<number[] | null>(null);
  const [isInstalling, setIsInstalling] = useState(false);

  const load = useCallback(async () => {
    try {
      // A rescan, not just the cached list: a JDK installed since the last
      // visit to the Java page should count.
      const found = await api.detectJavaInstallations();
      setInstalledMajors(
        found.map((j) => parseJavaMajor(j.version)).filter((m): m is number => m !== null),
      );
    } catch {
      setInstalledMajors(null);
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  async function install(major: number) {
    setIsInstalling(true);
    try {
      await api.installJava(major);
      toast.success(`Java ${major} installed`);
      await load();
    } catch (err) {
      toast.error(`Couldn't install Java ${major}`, { description: String(err) });
    } finally {
      setIsInstalling(false);
    }
  }

  if (required === null) return null;
  // Unknown (scan failed): say what's needed without claiming a status.
  if (installedMajors === null) {
    return <p className="text-xs text-muted-foreground">Requires Java {required}.</p>;
  }
  if (installedMajors.includes(required)) {
    return (
      <p className="flex items-center gap-1.5 text-xs text-muted-foreground">
        <CheckCircle2 className="size-3.5 text-green-600" />
        Java {required} is installed.
      </p>
    );
  }
  return (
    <div className="flex flex-wrap items-center gap-2 rounded-lg border border-amber-500/30 bg-amber-500/5 px-3 py-2 text-xs">
      <TriangleAlert className="size-3.5 shrink-0 text-amber-600" />
      <span className="min-w-0 flex-1 text-muted-foreground">
        Minecraft {minecraftVersion} needs Java {required}, which isn't installed.
      </span>
      {INSTALLABLE_MAJORS.includes(required) ? (
        <Button
          type="button"
          size="sm"
          variant="outline"
          disabled={isInstalling}
          onClick={() => void install(required)}
        >
          {isInstalling ? <Loader2 className="animate-spin" /> : <Download />}
          {isInstalling ? "Installing…" : `Install Java ${required}`}
        </Button>
      ) : (
        <span className="text-muted-foreground">Install it from the Java page.</span>
      )}
    </div>
  );
}
