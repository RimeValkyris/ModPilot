import { useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { ArrowLeft, Loader2, Search } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import {
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useInstancesStore } from "@/stores/instancesStore";
import { api, listenWithCleanup } from "@/lib/tauri";
import { INSTANCE_EXISTS_PREFIX } from "@/types/import";
import type { Instance } from "@/types/instance";
import {
  FTB_INSTALL_PROGRESS_EVENT,
  type FtbInstallProgress,
  type FtbPack,
  type FtbVersionPreview,
} from "@/types/ftb";

/** Search -> pick a version -> review -> install. */
type Stage = "search" | "review";

function formatSize(bytes: number): string {
  if (bytes <= 0) return "unknown size";
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${Math.max(1, Math.round(bytes / 1024 ** 2))} MB`;
}

interface FtbImportPanelProps {
  onBack: () => void;
  onInstalled: (instance: Instance) => void;
  /** Lets the parent dialog block its own close button while an install is
   * running - the download keeps going regardless, so closing mid-install
   * would just make it look like nothing happened. */
  onBusyChange: (busy: boolean) => void;
}

export function FtbImportPanel({ onBack, onInstalled, onBusyChange }: FtbImportPanelProps) {
  const { importFtbInstance } = useInstancesStore();

  const [stage, setStage] = useState<Stage>("search");
  const [query, setQuery] = useState("");
  const [isSearching, setIsSearching] = useState(false);
  const [results, setResults] = useState<FtbPack[]>([]);
  const [hasSearched, setHasSearched] = useState(false);

  const [pack, setPack] = useState<FtbPack | null>(null);
  const [versionId, setVersionId] = useState<string>("");
  const [preview, setPreview] = useState<FtbVersionPreview | null>(null);
  const [isAnalyzing, setIsAnalyzing] = useState(false);

  const [name, setName] = useState("");
  const [minRamMb, setMinRamMb] = useState("2048");
  const [maxRamMb, setMaxRamMb] = useState("4096");
  const [isInstalling, setIsInstalling] = useState(false);
  const [progress, setProgress] = useState<FtbInstallProgress | null>(null);
  const [conflictDirName, setConflictDirName] = useState<string | null>(null);

  // Guards against a slow search landing after a newer one - typing
  // quickly otherwise leaves the older, less relevant results on screen.
  const searchSeq = useRef(0);

  useEffect(() => {
    onBusyChange(isInstalling);
  }, [isInstalling, onBusyChange]);

  useEffect(() => {
    if (!isInstalling) return;
    return listenWithCleanup<FtbInstallProgress>(FTB_INSTALL_PROGRESS_EVENT, (event) => {
      setProgress(event.payload);
    });
  }, [isInstalling]);

  async function runSearch() {
    const term = query.trim();
    if (!term) return;
    const seq = ++searchSeq.current;
    setIsSearching(true);
    try {
      const packs = await api.searchFtbPacks(term);
      if (seq !== searchSeq.current) return;
      setResults(packs);
      setHasSearched(true);
    } catch (err) {
      if (seq !== searchSeq.current) return;
      toast.error("FTB search failed", { description: String(err) });
    } finally {
      if (seq === searchSeq.current) setIsSearching(false);
    }
  }

  async function selectPack(hit: FtbPack) {
    setIsAnalyzing(true);
    try {
      // The search result already carries versions, but re-fetching gets
      // them newest-first and guarantees a full record.
      const full = await api.getFtbPack(hit.id);
      const latest = full.versions[0];
      if (!latest) {
        toast.error("That pack has no published versions yet");
        return;
      }
      setPack(full);
      setName(full.name);
      setVersionId(String(latest.id));
      setStage("review");
      await loadPreview(full.id, latest.id);
    } catch (err) {
      toast.error("Failed to load that modpack", { description: String(err) });
    } finally {
      setIsAnalyzing(false);
    }
  }

  async function loadPreview(packId: number, version: number) {
    setIsAnalyzing(true);
    setPreview(null);
    try {
      setPreview(await api.analyzeFtbVersion(packId, version));
    } catch (err) {
      toast.error("Failed to read that version", { description: String(err) });
    } finally {
      setIsAnalyzing(false);
    }
  }

  async function install(overwrite: boolean) {
    if (!pack || !preview) return;
    if (!name.trim()) {
      toast.error("Instance name is required");
      return;
    }
    setIsInstalling(true);
    setProgress(null);
    try {
      const instance = await importFtbInstance({
        packId: pack.id,
        versionId: preview.versionId,
        name: name.trim(),
        minRamMb: Number(minRamMb) || undefined,
        maxRamMb: Number(maxRamMb) || undefined,
        overwrite,
      });
      toast.success(`Installed "${instance.name}"`);
      onInstalled(instance);
    } catch (err) {
      const message = String(err);
      if (message.includes(INSTANCE_EXISTS_PREFIX)) {
        setConflictDirName(message.split(INSTANCE_EXISTS_PREFIX)[1] ?? name);
      } else {
        toast.error("Failed to install the modpack", { description: message });
      }
    } finally {
      setIsInstalling(false);
    }
  }

  if (stage === "search") {
    return (
      <div className="flex flex-col gap-4">
        <DialogHeader>
          <DialogTitle>Install from FTB</DialogTitle>
          <DialogDescription>
            Search Feed the Beast's modpacks. ModpackPilot downloads the
            server files and installs the mod loader itself - you don't need
            FTB's server installer.
          </DialogDescription>
        </DialogHeader>

        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void runSearch();
          }}
        >
          <Input
            autoFocus
            value={query}
            placeholder="Search modpacks…"
            onChange={(e) => setQuery(e.target.value)}
          />
          <Button type="submit" variant="outline" disabled={isSearching || !query.trim()}>
            {isSearching ? <Loader2 className="animate-spin" /> : <Search />}
          </Button>
        </form>

        <div className="flex max-h-72 flex-col gap-1.5 overflow-y-auto">
          {results.map((hit) => (
            <button
              key={hit.id}
              type="button"
              disabled={isAnalyzing}
              onClick={() => void selectPack(hit)}
              className="flex items-center gap-3 rounded-lg border border-border px-3 py-2 text-left transition-colors hover:bg-muted/50 disabled:opacity-50"
            >
              {hit.iconUrl && (
                <img
                  src={hit.iconUrl}
                  alt=""
                  className="size-10 shrink-0 rounded-md object-cover"
                />
              )}
              <span className="min-w-0">
                <span className="block truncate text-sm font-medium">{hit.name}</span>
                <span className="block truncate text-xs text-muted-foreground">
                  {hit.synopsis}
                </span>
              </span>
            </button>
          ))}
          {hasSearched && results.length === 0 && !isSearching && (
            <p className="py-6 text-center text-sm text-muted-foreground">
              No modpacks matched that search.
            </p>
          )}
        </div>

        <DialogFooter>
          <Button type="button" variant="ghost" onClick={onBack}>
            <ArrowLeft />
            Back
          </Button>
        </DialogFooter>
      </div>
    );
  }

  return (
    <form
      className="flex flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        void install(false);
      }}
    >
      <DialogHeader>
        <DialogTitle>{pack?.name}</DialogTitle>
        <DialogDescription>
          Review what will be installed. Nothing is downloaded until you
          confirm.
        </DialogDescription>
      </DialogHeader>

      {isInstalling && (
        <div className="flex flex-col gap-1.5 rounded-lg border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
          <p>
            {progress?.phase === "installing-loader"
              ? (progress.detail ?? "Installing the mod loader…")
              : `Downloading ${progress?.done ?? 0} of ${progress?.total ?? preview?.totalFiles ?? 0} files…`}
          </p>
          <div className="h-1.5 overflow-hidden rounded-full bg-border">
            <div
              className="h-full bg-primary transition-[width]"
              style={{
                width: `${progress && progress.total > 0 ? Math.round((progress.done / progress.total) * 100) : 0}%`,
              }}
            />
          </div>
          <p>
            Large packs take a while, and the loader step runs its own
            installer at the end. Please don't close ModpackPilot.
          </p>
        </div>
      )}

      <fieldset disabled={isInstalling} className="contents">
        <div className="flex flex-col gap-1.5">
          <Label>Version</Label>
          <Select
            value={versionId}
            onValueChange={(v) => {
              if (!v) return;
              setVersionId(v);
              if (pack) void loadPreview(pack.id, Number(v));
            }}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {pack?.versions.map((v) => (
                <SelectItem key={v.id} value={String(v.id)}>
                  {v.name}
                  {v.type !== "release" ? ` (${v.type})` : ""}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        {isAnalyzing && (
          <p className="text-sm text-muted-foreground">Reading version details…</p>
        )}

        {preview && (
          <>
            <div className="flex flex-wrap gap-1.5">
              {preview.minecraftVersion && (
                <Badge variant="secondary">MC {preview.minecraftVersion}</Badge>
              )}
              <Badge variant="secondary">
                {preview.loader}
                {preview.loaderVersion ? ` ${preview.loaderVersion}` : ""}
              </Badge>
              <Badge variant="secondary">{preview.modCount} mods</Badge>
              <Badge variant="secondary">
                {preview.totalFiles} files · {formatSize(preview.downloadSizeBytes)}
              </Badge>
              {preview.javaMajor !== null && (
                <Badge variant="secondary">Java {preview.javaMajor}</Badge>
              )}
            </div>

            {preview.warnings.length > 0 && (
              <ul className="rounded-lg border border-yellow-500/30 bg-yellow-500/10 px-3 py-2 text-xs text-yellow-600 dark:text-yellow-400">
                {preview.warnings.map((warning) => (
                  <li key={warning}>{warning}</li>
                ))}
              </ul>
            )}
          </>
        )}

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="ftb-name">Name</Label>
          <Input id="ftb-name" value={name} onChange={(e) => setName(e.target.value)} />
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="ftb-min-ram">Min RAM (MB)</Label>
            <Input
              id="ftb-min-ram"
              type="number"
              min={512}
              step={512}
              value={minRamMb}
              onChange={(e) => setMinRamMb(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="ftb-max-ram">Max RAM (MB)</Label>
            <Input
              id="ftb-max-ram"
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
              Installing will permanently delete and replace it.
            </p>
            <Button
              type="button"
              variant="destructive"
              size="sm"
              onClick={() => void install(true)}
            >
              Overwrite and install
            </Button>
          </div>
        )}

        <DialogFooter>
          <Button
            type="button"
            variant="ghost"
            onClick={() => {
              setStage("search");
              setPreview(null);
              setConflictDirName(null);
            }}
          >
            <ArrowLeft />
            Back
          </Button>
          <Button type="submit" disabled={isInstalling || isAnalyzing || !preview}>
            {isInstalling ? "Installing…" : "Install"}
          </Button>
        </DialogFooter>
      </fieldset>
    </form>
  );
}
