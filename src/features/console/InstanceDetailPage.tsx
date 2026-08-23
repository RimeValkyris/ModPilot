import { useEffect } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useInstances } from "@/hooks/useInstances";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import { STATUS_DOT, STATUS_LABEL } from "@/lib/serverStatus";
import { Console } from "@/features/console/Console";
import { InstanceSettingsForm } from "@/features/settings/InstanceSettingsForm";
import { ResourceUsageRow } from "@/features/dashboard/ResourceUsageRow";
import { InstanceLogsTab } from "@/features/console/InstanceLogsTab";

const TABS = ["overview", "console", "files", "mods", "configuration", "logs"] as const;
type Tab = (typeof TABS)[number];

function ComingSoon({ label }: { label: string }) {
  return (
    <div className="rounded-xl border border-dashed border-border bg-card px-6 py-16 text-center text-sm text-muted-foreground">
      {label} is coming in a later phase.
    </div>
  );
}

export function InstanceDetailPage() {
  const { id, tab } = useParams<{ id: string; tab?: string }>();
  const navigate = useNavigate();
  const { instances, isLoading } = useInstances();
  const instance = instances.find((i) => i.id === id);
  const { wallpapers, fetchWallpaper } = useWallpaperStore();

  useEffect(() => {
    if (id) fetchWallpaper(id);
  }, [id, fetchWallpaper]);

  const activeTab: Tab = TABS.includes(tab as Tab) ? (tab as Tab) : "overview";

  if (isLoading && !instance) {
    return <div className="p-8 text-sm text-muted-foreground">Loading…</div>;
  }

  if (!instance) {
    return (
      <div className="flex flex-col gap-4 p-8">
        <p className="text-sm text-muted-foreground">Instance not found.</p>
        <Button variant="outline" className="w-fit" onClick={() => navigate("/servers")}>
          <ArrowLeft />
          Back to Servers
        </Button>
      </div>
    );
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-4 p-8">
      <Button
        variant="ghost"
        size="sm"
        className="w-fit"
        onClick={() => navigate("/servers")}
      >
        <ArrowLeft />
        Back to Servers
      </Button>

      <header
        className="flex items-center gap-3 rounded-xl p-4"
        style={
          wallpapers[instance.id]
            ? {
                backgroundImage: `linear-gradient(to bottom, rgba(0,0,0,0.25), rgba(0,0,0,0.55)), url(${wallpapers[instance.id]})`,
                backgroundSize: "cover",
                backgroundPosition: "center",
              }
            : undefined
        }
      >
        <h1
          className={`text-2xl font-semibold tracking-tight ${wallpapers[instance.id] ? "text-white" : ""}`}
        >
          {instance.name}
        </h1>
        <div className="flex items-center gap-1.5">
          <span className={`size-2 rounded-full ${STATUS_DOT[instance.status]}`} />
          <Badge variant="outline">{STATUS_LABEL[instance.status]}</Badge>
        </div>
      </header>

      <Tabs
        value={activeTab}
        onValueChange={(value) =>
          navigate(`/instances/${instance.id}${value === "overview" ? "" : `/${value}`}`)
        }
      >
        <TabsList>
          <TabsTrigger value="overview">Overview</TabsTrigger>
          <TabsTrigger value="console">Console</TabsTrigger>
          <TabsTrigger value="files">Files</TabsTrigger>
          <TabsTrigger value="mods">Mods</TabsTrigger>
          <TabsTrigger value="configuration">Configuration</TabsTrigger>
          <TabsTrigger value="logs">Logs</TabsTrigger>
        </TabsList>

        <TabsContent value="overview" className="flex flex-col gap-4">
          {instance.status === "running" && (
            <div className="rounded-xl border border-border bg-card p-4">
              <ResourceUsageRow instance={instance} />
            </div>
          )}
          <dl className="grid grid-cols-2 gap-x-6 gap-y-3 rounded-xl border border-border bg-card p-4 text-sm sm:grid-cols-3">
            <div>
              <dt className="text-muted-foreground">Minecraft version</dt>
              <dd>{instance.minecraftVersion ?? "Unknown"}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Loader</dt>
              <dd className="capitalize">{instance.loader}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Loader version</dt>
              <dd>{instance.loaderVersion ?? "Unknown"}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Server JAR</dt>
              <dd>{instance.serverJar ?? "Not set"}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Memory</dt>
              <dd>
                {instance.minRamMb} - {instance.maxRamMb} MB
              </dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Server directory</dt>
              <dd className="truncate" title={instance.serverDirectory}>
                {instance.serverDirectory}
              </dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Created</dt>
              <dd>{new Date(instance.createdAt).toLocaleString()}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Last launched</dt>
              <dd>
                {instance.lastLaunchedAt
                  ? new Date(instance.lastLaunchedAt).toLocaleString()
                  : "Never"}
              </dd>
            </div>
          </dl>
        </TabsContent>

        <TabsContent value="console">
          <Console instance={instance} />
        </TabsContent>

        <TabsContent value="files">
          <ComingSoon label="File management" />
        </TabsContent>
        <TabsContent value="mods">
          <ComingSoon label="Mod management" />
        </TabsContent>
        <TabsContent value="configuration">
          <InstanceSettingsForm instance={instance} />
        </TabsContent>
        <TabsContent value="logs">
          <InstanceLogsTab instance={instance} />
        </TabsContent>
      </Tabs>
    </div>
  );
}
