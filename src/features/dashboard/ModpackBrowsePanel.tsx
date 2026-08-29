import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { listen } from "@tauri-apps/api/event";
import { ArrowLeft, Loader2, Package, Search } from "lucide-react";
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
import { api } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { INSTANCE_EXISTS_PREFIX } from "@/types/import";
import type { Instance, ServerLoader } from "@/types/instance";
import {
  FTB_INSTALL_PROGRESS_EVENT,
  type FtbInstallProgress,
} from "@/types/ftb";

/** Browse/search -> pick a version -> review -> install. */
type Stage = "browse" | "review";

/** Which service a pack comes from. The two are interchangeable from here
 * on: both install as a new instance, both get linked for updates. */
type PackSource = "ftb" | "modrinth";

const SOURCE_LABELS: Record<PackSource, string> = {
  ftb: "Feed the Beast",
  modrinth: "Modrinth",
};

/** One search or browse result, flattened so the list doesn't care which
 * service it came from. `id` is the pack ID for FTB and the project ID for
 * Modrinth - both are only ever passed straight back to their own source. */
interface PackHit {
  source: PackSource;
  id: string;
  name: string;
  synopsis: string;
  iconUrl: string | null;
}

/** A version to offer in the picker. IDs are strings even for FTB, whose
 * are numeric, so one `<Select>` can drive both. */
interface VersionChoice {
  id: string;
  label: string;
}

/** The union of what either service can tell us about a version before
 * installing it. FTB reads a full manifest up front; Modrinth only knows
 * what its version metadata says, so the counts are absent there. */
interface PreviewView {
  minecraftVersion: string | null;
  loader: ServerLoader;
  loaderVersion: string | null;
  javaMajor: number | null;
  modCount: number | null;
  totalFiles: number | null;
  downloadSizeBytes: number | null;
  warnings: string[];
}

function formatSize(bytes: number): string {
  if (bytes <= 0) return "unknown size";
  const gb = bytes / 1024 ** 3;
  if (gb >= 1) return `${gb.toFixed(1)} GB`;
  return `${Math.max(1, Math.round(bytes / 1024 ** 2))} MB`;
}

interface ModpackBrowsePanelProps {
  onBack: () => void;
  onInstalled: (instance: Instance) => void;
  /** Lets the parent dialog block its own close button while an install is
   * running - the download keeps going regardless, so closing mid-install
   * would just make it look like nothing happened. */
  onBusyChange: (busy: boolean) => void;
}

export function ModpackBrowsePanel({
  onBack,
  onInstalled,
  onBusyChange,
}: ModpackBrowsePanelProps) {
  const { importFtbInstance, importModrinthInstance } = useInstancesStore();

  const [stage, setStage] = useState<Stage>("browse");
  const [source, setSource] = useState<PackSource>("ftb");
  const [query, setQuery] = useState("");
  const [isLoadingList, setIsLoadingList] = useState(false);
  const [results, setResults] = useState<PackHit[]>([]);
  const [listError, setListError] = useState<string | null>(null);
  // Pack art comes from someone else's CDN; a blocked or dead URL should
  // degrade to an icon rather than the browser's broken-image box.
  const [brokenIcons, setBrokenIcons] = useState<Record<string, boolean>>({});

  const [pack, setPack] = useState<PackHit | null>(null);
  const [versions, setVersions] = useState<VersionChoice[]>([]);
  const [versionId, setVersionId] = useState<string>("");
  const [preview, setPreview] = useState<PreviewView | null>(null);
  const [isAnalyzing, setIsAnalyzing] = useState(false);

  const [name, setName] = useState("");
  const [minRamMb, setMinRamMb] = useState("2048");
  const [maxRamMb, setMaxRamMb] = useState("4096");
  const [isInstalling, setIsInstalling] = useState(false);
  const [progress, setProgress] = useState<FtbInstallProgress | null>(null);
  const [conflictDirName, setConflictDirName] = useState<string | null>(null);

  // Guards against a slow request landing after a newer one - typing
  // quickly, or switching source mid-search, otherwise leaves the older
  // and less relevant results on screen.
  const listSeq = useRef(0);

  useEffect(() => {
    onBusyChange(isInstalling);
  }, [isInstalling, onBusyChange]);

  useEffect(() => {
    if (!isInstalling) return;
    let unlisten: (() => void) | undefined;
    listen<FtbInstallProgress>(FTB_INSTALL_PROGRESS_EVENT, (event) => {
      setProgress(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, [isInstalling]);

  /** Loads the list for a source: search results when there's a term,
   * otherwise the service's own browse list. */
  const loadList = useCallback(async (nextSource: PackSource, term: string) => {
    const seq = ++listSeq.current;
    setIsLoadingList(true);
    setListError(null);
    try {
      const hits: PackHit[] =
        nextSource === "ftb"
          ? (term ? await api.searchFtbPacks(term) : await api.browseFtbPacks()).map(
              (p) => ({
                source: "ftb" as const,
                id: String(p.id),
                name: p.name,
                synopsis: p.synopsis,
                iconUrl: p.iconUrl,
              }),
            )
          : (term
              ? await api.searchModrinthProjects(term)
              : await api.browseModrinthPacks()
            ).map((p) => ({
              source: "modrinth" as const,
              id: p.projectId,
              name: p.title,
              synopsis: p.description,
              iconUrl: p.iconUrl,
            }));
      if (seq !== listSeq.current) return;
      setResults(hits);
    } catch (err) {
      if (seq !== listSeq.current) return;
      setResults([]);
      setListError(String(err));
    } finally {
      if (seq === listSeq.current) setIsLoadingList(false);
    }
  }, []);

  // Fill the list on open and whenever the source changes, so the picker
  // always shows something to install rather than an empty box waiting on
  // a search term.
  useEffect(() => {
    void loadList(source, query.trim());
    // Deliberately not keyed on `query`: typing shouldn't fire a request
    // per keystroke - the search form submits instead.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [source, loadList]);

  async function selectPack(hit: PackHit) {
    setIsAnalyzing(true);
    try {
      const choices =
        hit.source === "ftb"
          ? // The browse/search result already carries versions, but
            // re-fetching gets them newest-first and guarantees a full record.
            (await api.getFtbPack(Number(hit.id))).versions.map((v) => ({
              id: String(v.id),
              label: v.type !== "release" ? `${v.name} (${v.type})` : v.name,
            }))
          : (await api.listModrinthProjectVersions(hit.id)).map((v) => ({
              id: v.id,
              label: v.versionNumber ? `${v.name} (${v.versionNumber})` : v.name,
            }));

      const latest = choices[0];
      if (!latest) {
        toast.error("That pack has no published versions yet");
        return;
      }

      setPack(hit);
      setName(hit.name);
      setVersions(choices);
      setVersionId(latest.id);
      setStage("review");
      await loadPreview(hit, latest.id);
    } catch (err) {
      toast.error("Failed to load that modpack", { description: String(err) });
    } finally {
      setIsAnalyzing(false);
    }
  }

  async function loadPreview(hit: PackHit, version: string) {
    setIsAnalyzing(true);
    setPreview(null);
    try {
      if (hit.source === "ftb") {
        const p = await api.analyzeFtbVersion(Number(hit.id), Number(version));
        setPreview({
          minecraftVersion: p.minecraftVersion,
          loader: p.loader,
          loaderVersion: p.loaderVersion,
          javaMajor: p.javaMajor,
          modCount: p.modCount,
          totalFiles: p.totalFiles,
          downloadSizeBytes: p.downloadSizeBytes,
          warnings: p.warnings,
        });
      } else {
        const p = await api.analyzeModrinthVersion(version);
        setPreview({
          minecraftVersion: p.minecraftVersion,
          loader: p.loader,
          // Modrinth's metadata names the loader but not its build, and
          // the pack's file list lives inside the .mrpack - see the
          // backend's ModrinthVersionPreview for why neither is fetched.
          loaderVersion: null,
          javaMajor: null,
          modCount: null,
          totalFiles: null,
          downloadSizeBytes: null,
          warnings: p.warnings,
        });
      }
    } catch (err) {
      toast.error("Failed to read that version", { description: String(err) });
    } finally {
      setIsAnalyzing(false);
    }
  }

  async function install(overwrite: boolean) {
    if (!pack || !preview || !versionId) return;
    if (!name.trim()) {
      toast.error("Instance name is required");
      return;
    }
    setIsInstalling(true);
    setProgress(null);
    try {
      const common = {
        name: name.trim(),
        minRamMb: Number(minRamMb) || undefined,
        maxRamMb: Number(maxRamMb) || undefined,
        overwrite,
      };
      const instance =
        pack.source === "ftb"
          ? await importFtbInstance({
              packId: Number(pack.id),
              versionId: Number(versionId),
              ...common,
            })
          : await importModrinthInstance({
              projectId: pack.id,
              versionId,
              ...common,
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

  if (stage === "browse") {
    return (
      <div className="flex min-w-0 flex-col gap-4">
        <DialogHeader>
          <DialogTitle>Install a modpack</DialogTitle>
          <DialogDescription>
            Browse Feed the Beast and Modrinth. ModpackPilot downloads the
            server files and installs the mod loader itself - you don't need
            the pack's own server installer.
          </DialogDescription>
        </DialogHeader>

        <div className="flex gap-1 rounded-lg bg-muted/50 p-1">
          {(Object.keys(SOURCE_LABELS) as PackSource[]).map((option) => (
            <button
              key={option}
              type="button"
              disabled={isAnalyzing}
              onClick={() => setSource(option)}
              className={cn(
                "flex-1 rounded-md px-3 py-1.5 text-sm font-medium transition-colors",
                source === option
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {SOURCE_LABELS[option]}
            </button>
          ))}
        </div>

        <form
          className="flex gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            void loadList(source, query.trim());
          }}
        >
          <Input
            autoFocus
            value={query}
            placeholder={`Search ${SOURCE_LABELS[source]}…`}
            onChange={(e) => setQuery(e.target.value)}
          />
          <Button type="submit" variant="outline" disabled={isLoadingList}>
            {isLoadingList ? <Loader2 className="animate-spin" /> : <Search />}
          </Button>
        </form>

        <div className="flex max-h-72 min-w-0 flex-col gap-1.5 overflow-y-auto">
          {results.map((hit) => {
            const iconKey = `${hit.source}:${hit.id}`;
            return (
              <button
                key={iconKey}
                type="button"
                disabled={isAnalyzing}
                onClick={() => void selectPack(hit)}
                className="flex w-full min-w-0 items-center gap-3 rounded-lg border border-border px-3 py-2 text-left transition-colors hover:bg-muted/50 disabled:opacity-50"
              >
                {hit.iconUrl && !brokenIcons[iconKey] ? (
                  <img
                    src={hit.iconUrl}
                    alt=""
                    className="size-10 shrink-0 rounded-md object-cover"
                    onError={() =>
                      setBrokenIcons((prev) => ({ ...prev, [iconKey]: true }))
                    }
                  />
                ) : (
                  <Package className="size-10 shrink-0 rounded-md bg-muted p-2 text-muted-foreground" />
                )}
                <span className="block min-w-0 flex-1">
                  <span className="block truncate text-sm font-medium">{hit.name}</span>
                  <span className="block truncate text-xs text-muted-foreground">
                    {hit.synopsis}
                  </span>
                </span>
              </button>
            );
          })}

          {isLoadingList && results.length === 0 && (
            <p className="py-6 text-center text-sm text-muted-foreground">
              Loading modpacks…
            </p>
          )}
          {listError && !isLoadingList && (
            <p className="py-6 text-center text-sm text-destructive">{listError}</p>
          )}
          {!isLoadingList && !listError && results.length === 0 && (
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
      className="flex min-w-0 flex-col gap-4"
      onSubmit={(e) => {
        e.preventDefault();
        void install(false);
      }}
    >
      <DialogHeader>
        <DialogTitle className="truncate">{pack?.name}</DialogTitle>
        <DialogDescription>
          Review what will be installed from{" "}
          {pack ? SOURCE_LABELS[pack.source] : "the pack's source"}. Nothing is
          downloaded until you confirm.
        </DialogDescription>
      </DialogHeader>

      {isInstalling && (
        <div className="flex flex-col gap-1.5 rounded-lg border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
          <p>
            {progress?.phase === "installing-loader"
              ? (progress.detail ?? "Installing the mod loader…")
              : progress && progress.total > 0
                ? `Downloading ${progress.done} of ${progress.total} files…`
                : "Downloading the pack…"}
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
        <div className="flex min-w-0 flex-col gap-1.5">
          <Label>Version</Label>
          <Select
            value={versionId}
            onValueChange={(v) => {
              if (!v) return;
              setVersionId(v);
              if (pack) void loadPreview(pack, v);
            }}
          >
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              {versions.map((v) => (
                <SelectItem key={v.id} value={v.id}>
                  {v.label}
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
              {preview.modCount !== null && (
                <Badge variant="secondary">{preview.modCount} mods</Badge>
              )}
              {preview.totalFiles !== null && preview.downloadSizeBytes !== null && (
                <Badge variant="secondary">
                  {preview.totalFiles} files · {formatSize(preview.downloadSizeBytes)}
                </Badge>
              )}
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

        <div className="flex min-w-0 flex-col gap-1.5">
          <Label htmlFor="pack-name">Name</Label>
          <Input id="pack-name" value={name} onChange={(e) => setName(e.target.value)} />
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div className="flex min-w-0 flex-col gap-1.5">
            <Label htmlFor="pack-min-ram">Min RAM (MB)</Label>
            <Input
              id="pack-min-ram"
              type="number"
              min={512}
              step={512}
              value={minRamMb}
              onChange={(e) => setMinRamMb(e.target.value)}
            />
          </div>
          <div className="flex min-w-0 flex-col gap-1.5">
            <Label htmlFor="pack-max-ram">Max RAM (MB)</Label>
            <Input
              id="pack-max-ram"
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
              setStage("browse");
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
