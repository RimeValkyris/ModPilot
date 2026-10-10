import { useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import {
  Check,
  ChevronDown,
  ChevronUp,
  Download,
  Loader2,
  Package,
  RefreshCw,
  Search,
  SlidersHorizontal,
  Trash2,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
import { api } from "@/lib/tauri";
import { formatFileSize } from "@/lib/format";
import type { Instance } from "@/types/instance";
import type { ModInfo } from "@/types/mod";
import type { ModrinthSearchHit, ModrinthVersion } from "@/types/modrinth";

type PluginFilter = "all" | "enabled" | "disabled";

export function InstancePluginsTab({ instance }: { instance: Instance }) {
  const [plugins, setPlugins] = useState<ModInfo[]>([]);
  const [query, setQuery] = useState("");
  const [submittedQuery, setSubmittedQuery] = useState("");
  const [results, setResults] = useState<ModrinthSearchHit[]>([]);
  const [versions, setVersions] = useState<Record<string, ModrinthVersion[]>>({});
  const [expandedProject, setExpandedProject] = useState<string | null>(null);
  const [filter, setFilter] = useState<PluginFilter>("all");
  const [isLoading, setIsLoading] = useState(true);
  const [isSearching, setIsSearching] = useState(false);
  const [installingVersion, setInstallingVersion] = useState<string | null>(null);
  const [deletingFile, setDeletingFile] = useState<string | null>(null);

  const visiblePlugins = useMemo(
    () => plugins.filter((plugin) => filter === "all" || (filter === "enabled" ? plugin.enabled : !plugin.enabled)),
    [filter, plugins],
  );
  const enabledCount = plugins.filter((plugin) => plugin.enabled).length;

  async function refresh(showLoading = false) {
    if (showLoading) setIsLoading(true);
    try {
      setPlugins(await api.listPlugins(instance.id));
    } catch (err) {
      toast.error("Failed to load plugins", { description: String(err) });
    } finally {
      if (showLoading) setIsLoading(false);
    }
  }

  useEffect(() => {
    void refresh(true);
  }, [instance.id]);

  // Results are live: popular plugins show as soon as the tab opens, and
  // typing re-searches after a short pause (no Search click needed). An empty
  // box means "popular". `cancelled` drops a slow response that a newer
  // keystroke has already superseded, so results never flicker back.
  useEffect(() => {
    const trimmed = query.trim();
    let cancelled = false;
    const timer = window.setTimeout(
      async () => {
        setIsSearching(true);
        setSubmittedQuery(trimmed);
        try {
          const hits = await api.searchModrinthPlugins(instance.id, trimmed);
          if (cancelled) return;
          setResults(hits);
          setExpandedProject(null);
        } catch (err) {
          if (!cancelled) toast.error("Plugin search failed", { description: String(err) });
        } finally {
          if (!cancelled) setIsSearching(false);
        }
      },
      trimmed ? 350 : 0,
    );
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query, instance.id, instance.minecraftVersion, instance.loader]);

  async function loadVersions(projectId: string) {
    if (expandedProject === projectId) {
      setExpandedProject(null);
      return;
    }
    setExpandedProject(projectId);
    if (versions[projectId]) return;
    try {
      const next = await api.listModrinthProjectVersions(projectId);
      setVersions((current) => ({ ...current, [projectId]: next }));
    } catch (err) {
      setExpandedProject(null);
      toast.error("Failed to load plugin versions", { description: String(err) });
    }
  }

  async function install(version: ModrinthVersion) {
    const file = version.files.find((candidate) => candidate.primary) ?? version.files[0];
    if (!file) {
      toast.error("This version has no downloadable plugin JAR");
      return;
    }
    setInstallingVersion(version.id);
    try {
      await api.installModrinthPlugin(instance.id, version.id, file.filename);
      toast.success(`Installed ${file.filename}`);
      await refresh();
    } catch (err) {
      toast.error("Plugin installation failed", { description: String(err) });
    } finally {
      setInstallingVersion(null);
    }
  }

  async function toggle(plugin: ModInfo) {
    try {
      await api.togglePlugin(instance.id, plugin.fileName);
      await refresh();
    } catch (err) {
      toast.error("Failed to toggle plugin", { description: String(err) });
    }
  }

  async function remove(plugin: ModInfo) {
    if (!window.confirm(`Delete ${plugin.displayName}?`)) return;
    setDeletingFile(plugin.fileName);
    try {
      await api.deletePlugin(instance.id, plugin.fileName);
      setPlugins((current) => current.filter((item) => item.fileName !== plugin.fileName));
      toast.success("Plugin deleted");
    } catch (err) {
      toast.error("Failed to delete plugin", { description: String(err) });
    } finally {
      setDeletingFile(null);
    }
  }

  function isCompatible(version: ModrinthVersion) {
    const minecraftMatches =
      !instance.minecraftVersion || version.gameVersions.length === 0
        ? true
        : version.gameVersions.includes(instance.minecraftVersion);
    const paperCompatible =
      version.loaders.length === 0 ||
      version.loaders.some((loader) => ["paper", "spigot", "bukkit"].includes(loader.toLowerCase()));
    return minecraftMatches && paperCompatible;
  }

  return (
    <div className="flex flex-col gap-4">
      <Card>
        <CardHeader className="border-b">
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <CardTitle className="flex items-center gap-2">
                <Package className="size-4 text-primary" />
                Installed plugins
              </CardTitle>
              <CardDescription className="mt-1">
                Changes apply the next time this server starts.
              </CardDescription>
            </div>
            <div className="flex items-center gap-2">
              <Badge variant="secondary">{plugins.length} total</Badge>
              <Badge variant="outline">{enabledCount} enabled</Badge>
              <Button
                variant="ghost"
                size="icon-sm"
                title="Refresh plugins"
                aria-label="Refresh plugins"
                onClick={() => void refresh(true)}
                disabled={isLoading}
              >
                <RefreshCw className={isLoading ? "animate-spin" : ""} />
              </Button>
            </div>
          </div>
        </CardHeader>
        <CardContent className="pt-4">
          <div className="mb-3 flex flex-wrap gap-2">
            {(["all", "enabled", "disabled"] as const).map((option) => (
              <Button
                key={option}
                size="sm"
                variant={filter === option ? "secondary" : "ghost"}
                onClick={() => setFilter(option)}
              >
                {option === "all" ? "All" : option === "enabled" ? "Enabled" : "Disabled"}
              </Button>
            ))}
            <span className="ml-auto flex items-center gap-1 text-xs text-muted-foreground">
              <SlidersHorizontal className="size-3.5" />
              {visiblePlugins.length} shown
            </span>
          </div>

          {isLoading ? (
            <div className="flex items-center justify-center gap-2 rounded-lg border border-dashed p-8 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" /> Loading plugins…
            </div>
          ) : visiblePlugins.length === 0 ? (
            <div className="flex flex-col items-center gap-2 rounded-lg border border-dashed p-8 text-center">
              <Package className="size-8 text-muted-foreground/60" />
              <p className="text-sm font-medium">
                {plugins.length === 0 ? "No plugins installed" : `No ${filter} plugins`}
              </p>
              <p className="max-w-sm text-xs text-muted-foreground">
                {plugins.length === 0
                  ? "Search Modrinth below to find Paper plugins for this server."
                  : "Try another filter to see the rest of this server's plugins."}
              </p>
            </div>
          ) : (
            <ul className="flex flex-col gap-2">
              {visiblePlugins.map((plugin) => (
                <li
                  key={plugin.fileName}
                  className="flex items-center justify-between gap-3 rounded-lg border border-border p-3 transition-colors hover:bg-muted/40"
                >
                  <div className="flex min-w-0 items-center gap-3">
                    <Switch
                      checked={plugin.enabled}
                      aria-label={`${plugin.enabled ? "Disable" : "Enable"} ${plugin.displayName}`}
                      onCheckedChange={() => void toggle(plugin)}
                    />
                    <div className="min-w-0">
                      <p className="truncate text-sm font-medium" title={plugin.displayName}>
                        {plugin.displayName}
                      </p>
                      <p className="text-xs text-muted-foreground">{formatFileSize(plugin.sizeBytes)}</p>
                    </div>
                    {!plugin.enabled && <Badge variant="outline">Disabled</Badge>}
                  </div>
                  <Button
                    variant="ghost"
                    size="icon-sm"
                    aria-label={`Delete ${plugin.displayName}`}
                    disabled={deletingFile === plugin.fileName}
                    onClick={() => void remove(plugin)}
                  >
                    {deletingFile === plugin.fileName ? <Loader2 className="animate-spin" /> : <Trash2 />}
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="border-b">
          <CardTitle>Find Paper plugins</CardTitle>
          <CardDescription>
            Search Modrinth and install a verified plugin JAR directly into this server.
          </CardDescription>
        </CardHeader>
        <CardContent className="pt-4">
          <div className="flex flex-col gap-2 sm:flex-row">
            <div className="relative flex-1">
              <Search className="pointer-events-none absolute left-3 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" />
              <Input
                className="pl-9 pr-9"
                placeholder="Try Essentials, LuckPerms, ViaVersion…"
                value={query}
                onChange={(event) => setQuery(event.target.value)}
              />
              {query && (
                <button
                  type="button"
                  className="absolute right-3 top-1/2 -translate-y-1/2 text-muted-foreground hover:text-foreground"
                  aria-label="Clear plugin search"
                  onClick={() => setQuery("")}
                >
                  <X className="size-4" />
                </button>
              )}
            </div>
            {isSearching && (
              <span className="flex items-center gap-2 text-sm text-muted-foreground">
                <Loader2 className="size-4 animate-spin" /> Searching…
              </span>
            )}
          </div>

          {results.length === 0 && isSearching ? (
            <div className="flex items-center justify-center gap-2 p-8 text-sm text-muted-foreground">
              <Loader2 className="size-4 animate-spin" /> Loading plugins…
            </div>
          ) : submittedQuery && results.length === 0 ? (
            <div className="p-8 text-center text-sm text-muted-foreground">
              No plugins found for <strong className="text-foreground">“{submittedQuery}”</strong>.
            </div>
          ) : results.length > 0 ? (
            <div className="mt-4 flex flex-col gap-2">
              <p className="text-xs text-muted-foreground">
                {submittedQuery ? `Results for “${submittedQuery}”` : "Most downloaded plugins"}
              </p>
              {results.map((project) => {
                const projectVersions = (versions[project.projectId] ?? []).filter(isCompatible);
                const isExpanded = expandedProject === project.projectId;
                return (
                  <div key={project.projectId} className="rounded-lg border border-border">
                    <div className="flex items-center gap-3 p-3">
                      {project.iconUrl ? (
                        <img src={project.iconUrl} alt="" className="size-10 rounded-lg object-cover" />
                      ) : (
                        <div className="flex size-10 shrink-0 items-center justify-center rounded-lg bg-muted">
                          <Package className="size-5 text-muted-foreground" />
                        </div>
                      )}
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-sm font-medium">{project.title}</p>
                        <p className="truncate text-xs text-muted-foreground">{project.description}</p>
                      </div>
                      <Button variant="outline" size="sm" onClick={() => void loadVersions(project.projectId)}>
                        {isExpanded ? <ChevronUp /> : <ChevronDown />}
                        {isExpanded ? "Hide" : "Versions"}
                      </Button>
                    </div>
                    {isExpanded && (
                      <div className="border-t border-border bg-muted/20 p-3">
                        {projectVersions.length === 0 ? (
                          <p className="text-xs text-muted-foreground">
                            No Paper version matches Minecraft {instance.minecraftVersion ?? "this instance"}.
                          </p>
                        ) : (
                          <div className="flex flex-col gap-2">
                            {projectVersions.slice(0, 5).map((version) => (
                              <div key={version.id} className="flex flex-wrap items-center justify-between gap-2 rounded-md bg-background p-2 text-sm">
                                <div className="flex items-center gap-2">
                                  <span>{version.name}</span>
                                  <Badge variant="outline">{version.versionNumber}</Badge>
                                  {version.id === projectVersions[0].id && (
                                    <Badge><Check /> Latest</Badge>
                                  )}
                                </div>
                                <Button
                                  size="sm"
                                  disabled={installingVersion !== null}
                                  onClick={() => void install(version)}
                                >
                                  {installingVersion === version.id ? <Loader2 className="animate-spin" /> : <Download />}
                                  {installingVersion === version.id ? "Installing…" : "Install"}
                                </Button>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          ) : (
            <div className="flex flex-col items-center gap-2 p-8 text-center text-muted-foreground">
              <Search className="size-8 opacity-50" />
              <p className="text-sm">No plugins to show.</p>
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
