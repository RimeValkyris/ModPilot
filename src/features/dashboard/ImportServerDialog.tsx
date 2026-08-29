import { useEffect, useState } from "react";
import { toast } from "sonner";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { Boxes, FileArchive, Folder, PackagePlus, UploadCloud } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useInstancesStore } from "@/stores/instancesStore";
import { api } from "@/lib/tauri";
import { getRequiredJavaMajor } from "@/lib/javaRequirement";
import { SERVER_LOADERS, type ServerLoader } from "@/types/instance";
import {
  INSTANCE_EXISTS_PREFIX,
  type DetectedServerInfo,
  type ImportSource,
} from "@/types/import";
import { FtbImportPanel } from "./FtbImportPanel";

const LOADER_LABELS: Record<ServerLoader, string> = {
  vanilla: "Vanilla",
  forge: "Forge",
  neoforge: "NeoForge",
  fabric: "Fabric",
  quilt: "Quilt",
  unknown: "Unknown",
};

type Step = "select" | "analyzing" | "review" | "ftb";

function sourceLabel(source: ImportSource): string {
  const fileName = source.path.split(/[/\\]/).pop() ?? source.path;
  return source.kind === "zip" ? `${fileName} (ZIP)` : `${fileName} (folder)`;
}

export function ImportServerDialog() {
  const { importInstance } = useInstancesStore();
  const [open, setOpen] = useState(false);
  const [step, setStep] = useState<Step>("select");
  const [source, setSource] = useState<ImportSource | null>(null);
  const [detected, setDetected] = useState<DetectedServerInfo | null>(null);
  const [conflictDirName, setConflictDirName] = useState<string | null>(null);
  const [isSubmitting, setIsSubmitting] = useState(false);

  const [name, setName] = useState("");
  const [minecraftVersion, setMinecraftVersion] = useState("");
  const [loader, setLoader] = useState<ServerLoader>("unknown");
  const [minRamMb, setMinRamMb] = useState("2048");
  const [maxRamMb, setMaxRamMb] = useState("4096");

  function reset() {
    setStep("select");
    setSource(null);
    setDetected(null);
    setConflictDirName(null);
    setName("");
    setMinecraftVersion("");
    setLoader("unknown");
    setMinRamMb("2048");
    setMaxRamMb("4096");
  }

  async function runAnalysis(nextSource: ImportSource) {
    setSource(nextSource);
    setStep("analyzing");
    try {
      const result = await api.analyzeImport(nextSource);
      setDetected(result);
      setMinecraftVersion(result.minecraftVersion ?? "");
      setLoader(result.loader);
      const fileName = nextSource.path.split(/[/\\]/).pop() ?? "";
      setName(fileName.replace(/\.zip$/i, ""));
      setStep("review");
    } catch (err) {
      toast.error("Failed to analyze server files", { description: String(err) });
      setStep("select");
      setSource(null);
    }
  }

  async function handleSelectZip() {
    const path = await openDialog({
      title: "Select a server ZIP file",
      multiple: false,
      directory: false,
      filters: [{ name: "Server ZIP", extensions: ["zip"] }],
    });
    if (typeof path === "string") {
      await runAnalysis({ kind: "zip", path });
    }
  }

  async function handleSelectFolder() {
    const path = await openDialog({
      title: "Select a server folder",
      multiple: false,
      directory: true,
    });
    if (typeof path === "string") {
      await runAnalysis({ kind: "folder", path });
    }
  }

  // Drag-and-drop support: only active while this dialog is on the
  // "select source" step, so a drop elsewhere in the app isn't hijacked.
  useEffect(() => {
    if (!open || step !== "select") return;

    let unlisten: (() => void) | undefined;
    getCurrentWebview()
      .onDragDropEvent((event) => {
        if (event.payload.type !== "drop") return;
        const droppedPath = event.payload.paths[0];
        if (!droppedPath) return;
        const nextSource: ImportSource = droppedPath.toLowerCase().endsWith(".zip")
          ? { kind: "zip", path: droppedPath }
          : { kind: "folder", path: droppedPath };
        void runAnalysis(nextSource);
      })
      .then((fn) => {
        unlisten = fn;
      });

    return () => unlisten?.();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, step]);

  async function submit(overwrite: boolean) {
    if (!source) return;
    if (!name.trim()) {
      toast.error("Instance name is required");
      return;
    }

    setIsSubmitting(true);
    try {
      const instance = await importInstance(source, {
        name: name.trim(),
        minecraftVersion: minecraftVersion.trim() || null,
        loader,
        minRamMb: Number(minRamMb) || undefined,
        maxRamMb: Number(maxRamMb) || undefined,
        overwrite,
      });
      toast.success(`Imported "${instance.name}"`);
      setOpen(false);
      reset();
    } catch (err) {
      const message = String(err);
      if (message.includes(INSTANCE_EXISTS_PREFIX)) {
        setConflictDirName(message.split(INSTANCE_EXISTS_PREFIX)[1] ?? name);
      } else {
        toast.error("Failed to import instance", { description: message });
      }
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        // Block closing (Escape, backdrop click, the corner X) while an
        // import is actually running - the extraction/copy keeps going in
        // the background regardless, so closing here just makes it look
        // like nothing happened while a large modpack is still mid-copy.
        if (!next && isSubmitting) return;
        setOpen(next);
        if (!next) reset();
      }}
    >
      <DialogTrigger render={<Button variant="outline" />}>
        <PackagePlus />
        Import Server
      </DialogTrigger>
      <DialogContent className="sm:max-w-md" showCloseButton={!isSubmitting}>
        {step === "select" && (
          <div className="flex flex-col gap-4">
            <DialogHeader>
              <DialogTitle>Import Server</DialogTitle>
              <DialogDescription>
                Import an existing Minecraft server from a ZIP file or a
                folder, or install an FTB modpack directly. Nothing is
                copied until you review what was detected.
              </DialogDescription>
            </DialogHeader>

            <label className="flex flex-col items-center justify-center gap-2 rounded-xl border border-dashed border-border bg-muted/30 px-6 py-8 text-center text-sm text-muted-foreground">
              <UploadCloud className="size-6" />
              Drag and drop a server ZIP file here
            </label>

            <div className="flex gap-3">
              <Button variant="outline" className="flex-1" onClick={handleSelectZip}>
                <FileArchive />
                Select ZIP
              </Button>
              <Button variant="outline" className="flex-1" onClick={handleSelectFolder}>
                <Folder />
                Select Folder
              </Button>
            </div>

            <Button variant="outline" onClick={() => setStep("ftb")}>
              <Boxes />
              Install an FTB modpack
            </Button>
          </div>
        )}

        {step === "ftb" && (
          <FtbImportPanel
            onBack={() => setStep("select")}
            onInstalled={() => {
              setOpen(false);
              reset();
            }}
            onBusyChange={setIsSubmitting}
          />
        )}

        {step === "analyzing" && (
          <div className="flex flex-col items-center gap-3 py-10 text-sm text-muted-foreground">
            <DialogHeader className="sr-only">
              <DialogTitle>Analyzing</DialogTitle>
            </DialogHeader>
            Analyzing server files…
          </div>
        )}

        {step === "review" && source && detected && (
          <form
            onSubmit={(e) => {
              e.preventDefault();
              submit(false);
            }}
            className="flex flex-col gap-4"
          >
            <DialogHeader>
              <DialogTitle>Review detected server</DialogTitle>
              <DialogDescription>{sourceLabel(source)}</DialogDescription>
            </DialogHeader>

            {isSubmitting && (
              <p className="rounded-lg border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
                Copying files… large modpacks can take a while. This dialog
                will close automatically when it's done - please don't close
                ModpackPilot in the meantime.
              </p>
            )}

            <fieldset disabled={isSubmitting} className="contents">
            <div className="flex flex-wrap gap-1.5">
              {detected.serverJar && <Badge variant="secondary">JAR: {detected.serverJar}</Badge>}
              {detected.hasModsFolder && (
                <Badge variant="secondary">{detected.modCount} mods</Badge>
              )}
              {detected.hasWorldFolder && (
                <Badge variant="secondary">World: {detected.worldFolderName}</Badge>
              )}
              {detected.hasConfigFolder && <Badge variant="secondary">config/</Badge>}
              {detected.hasServerProperties && (
                <Badge variant="secondary">server.properties</Badge>
              )}
              {detected.startScripts.map((script) => (
                <Badge key={script} variant="secondary">
                  {script}
                </Badge>
              ))}
            </div>

            {detected.warnings.length > 0 && (
              <ul className="rounded-lg border border-yellow-500/30 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-600 dark:text-yellow-400">
                {detected.warnings.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
            )}

            <div className="flex flex-col gap-1.5">
              <Label htmlFor="import-name">Name</Label>
              <Input
                id="import-name"
                autoFocus
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </div>

            <div className="flex flex-col gap-1.5">
              <Label htmlFor="import-mc-version">Minecraft version</Label>
              <Input
                id="import-mc-version"
                value={minecraftVersion}
                onChange={(e) => setMinecraftVersion(e.target.value)}
                placeholder="Unknown"
              />
              {getRequiredJavaMajor(minecraftVersion) !== null && (
                <p className="text-xs text-muted-foreground">
                  Requires Java {getRequiredJavaMajor(minecraftVersion)}. You can assign it after
                  importing.
                </p>
              )}
            </div>

            <div className="flex flex-col gap-1.5">
              <Label>Loader</Label>
              <Select value={loader} onValueChange={(v) => setLoader(v as ServerLoader)}>
                <SelectTrigger className="w-full">
                  <SelectValue />
                </SelectTrigger>
                <SelectContent>
                  {[...SERVER_LOADERS, "unknown" as const].map((l) => (
                    <SelectItem key={l} value={l}>
                      {LOADER_LABELS[l]}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>

            <div className="grid grid-cols-2 gap-3">
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="import-min-ram">Min RAM (MB)</Label>
                <Input
                  id="import-min-ram"
                  type="number"
                  min={512}
                  step={512}
                  value={minRamMb}
                  onChange={(e) => setMinRamMb(e.target.value)}
                />
              </div>
              <div className="flex flex-col gap-1.5">
                <Label htmlFor="import-max-ram">Max RAM (MB)</Label>
                <Input
                  id="import-max-ram"
                  type="number"
                  min={512}
                  step={512}
                  value={maxRamMb}
                  onChange={(e) => setMaxRamMb(e.target.value)}
                />
              </div>
            </div>

            {conflictDirName && (
              <div className="flex flex-col gap-2 rounded-lg border border-destructive/30 bg-destructive/10 px-3 py-2 text-sm text-destructive">
                <p>
                  An instance folder named "{conflictDirName}" already exists.
                  Importing will permanently delete and replace it.
                </p>
                <Button
                  type="button"
                  variant="destructive"
                  size="sm"
                  disabled={isSubmitting}
                  onClick={() => submit(true)}
                >
                  Overwrite and import
                </Button>
              </div>
            )}

            <DialogFooter>
              <Button type="submit" disabled={isSubmitting}>
                {isSubmitting ? "Importing…" : "Import"}
              </Button>
            </DialogFooter>
            </fieldset>
          </form>
        )}
      </DialogContent>
    </Dialog>
  );
}
