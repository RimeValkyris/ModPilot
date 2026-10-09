import { useEffect } from "react";
import { toast } from "sonner";
import { Coffee, Download, RefreshCw, Star } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { useJavaStore } from "@/stores/javaStore";

export function JavaPage() {
  const { installations, isLoading, isScanning, error, fetchInstallations, rescan, install, setDefault } =
    useJavaStore();

  useEffect(() => {
    fetchInstallations();
  }, [fetchInstallations]);

  async function handleRescan() {
    try {
      await rescan();
      toast.success("Java scan complete");
    } catch (err) {
      toast.error("Java scan failed", { description: String(err) });
    }
  }

  async function handleSetDefault(id: string) {
    try {
      await setDefault(id);
    } catch (err) {
      toast.error("Failed to set default Java", { description: String(err) });
    }
  }

  async function handleInstall(major: number) {
    try {
      await install(major);
      toast.success(`Java ${major} installed`, {
        description: "The installation was added to ModpackPilot.",
      });
    } catch (err) {
      toast.error(`Java ${major} installation failed`, { description: String(err) });
    }
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 p-8">
      <header className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Java</h1>
          <p className="text-sm text-muted-foreground">
            Java installations ModpackPilot detected on this machine.
          </p>
        </div>
        <Button variant="outline" onClick={handleRescan} disabled={isScanning}>
          <RefreshCw className={isScanning ? "animate-spin" : ""} />
          {isScanning ? "Scanning…" : "Rescan"}
        </Button>
      </header>

      <section className="rounded-xl border border-border bg-card p-5">
        <div className="mb-4">
          <h2 className="font-medium">Install a JDK</h2>
          <p className="mt-1 text-sm text-muted-foreground">
            Downloads Eclipse Temurin from Adoptium and installs it privately for ModpackPilot.
          </p>
        </div>
        <div className="flex flex-wrap gap-2">
          {[8, 17, 21, 25].map((major) => (
            <Button key={major} variant="outline" onClick={() => handleInstall(major)} disabled={isScanning}>
              <Download />
              Java {major}
            </Button>
          ))}
        </div>
        <p className="mt-3 text-xs text-muted-foreground">
          Windows x64 and ARM64 are supported. Existing system Java installations are not changed.
        </p>
      </section>

      {error && (
        <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive">
          {error}
        </div>
      )}

      {isLoading ? (
        <div className="rounded-xl border border-border bg-card px-6 py-16 text-center text-sm text-muted-foreground">
          Loading…
        </div>
      ) : installations.length === 0 ? (
        <div className="flex flex-col items-center gap-3 rounded-xl border border-dashed border-border bg-card px-6 py-16 text-center">
          <Coffee className="size-8 text-muted-foreground" />
          <p className="text-base font-medium">No Java installations found</p>
          <p className="max-w-sm text-sm text-muted-foreground">
            Install a JDK (e.g. Eclipse Temurin), then click Rescan.
          </p>
        </div>
      ) : (
        <ul className="flex flex-col gap-2">
          {installations.map((java) => (
            <li
              key={java.id}
              className="flex items-center justify-between gap-4 rounded-xl border border-border bg-card p-4"
            >
              <div>
                <div className="flex items-center gap-2">
                  <p className="font-medium">Java {java.version}</p>
                  {java.vendor && <Badge variant="secondary">{java.vendor}</Badge>}
                  <Badge variant="outline">{java.architecture}</Badge>
                  {java.isDefault && (
                    <Badge>
                      <Star className="size-3" />
                      Default
                    </Badge>
                  )}
                </div>
                <p className="mt-1 truncate text-sm text-muted-foreground">{java.path}</p>
              </div>
              {!java.isDefault && (
                <Button variant="outline" size="sm" onClick={() => handleSetDefault(java.id)}>
                  Set as default
                </Button>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
