import { useNavigate, useParams } from "react-router-dom";
import { ArrowLeft } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useInstances } from "@/hooks/useInstances";
import { useInstanceAvatar } from "@/hooks/useInstanceAvatar";
import { STATUS_BADGE_CLASS, STATUS_LABEL } from "@/lib/serverStatus";
import { ServerAvatarPlaceholder } from "@/components/ServerAvatarPlaceholder";
import { Console } from "@/features/console/Console";
import { ForgeInstallBanner } from "@/features/console/ForgeInstallBanner";
import { InstanceSettingsForm } from "@/features/settings/InstanceSettingsForm";
import { ServerPropertiesForm } from "@/features/settings/ServerPropertiesForm";
import { ModpackUpdatesCard } from "@/features/settings/ModpackUpdatesCard";
import { AutomationCard } from "@/features/settings/AutomationCard";
import { InstanceLogsTab } from "@/features/console/InstanceLogsTab";
import { InstanceFilesTab } from "@/features/console/InstanceFilesTab";
import { InstanceModsTab } from "@/features/console/InstanceModsTab";
import { InstancePlayersTab } from "@/features/console/InstancePlayersTab";
import { OnlinePlayersCard } from "@/features/console/OnlinePlayersCard";
import { InstanceDashboard } from "@/features/console/InstanceDashboard";
import { DiagnosticsTab } from "@/features/console/DiagnosticsTab";
import { PerformanceHistoryCard } from "@/features/console/PerformanceHistoryCard";

const TABS = [
  "overview",
  "console",
  "files",
  "mods",
  "players",
  "configuration",
  "diagnostics",
  "logs",
] as const;
type Tab = (typeof TABS)[number];

export function InstanceDetailPage() {
  const { id, tab } = useParams<{ id: string; tab?: string }>();
  const navigate = useNavigate();
  const { instances, isLoading } = useInstances();
  const instance = instances.find((i) => i.id === id);

  const activeTab: Tab = TABS.includes(tab as Tab) ? (tab as Tab) : "overview";

  const [avatar] = useInstanceAvatar(instance?.id);

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

      <header className="flex items-center gap-3 rounded-xl p-4">
        {avatar ? (
          <img
            src={avatar}
            alt=""
            className="size-10 shrink-0 rounded-lg border border-border object-cover"
          />
        ) : (
          <ServerAvatarPlaceholder className="size-10 shrink-0 rounded-lg border border-border" />
        )}
        <h1 className="text-2xl font-semibold tracking-tight">{instance.name}</h1>
        <Badge className={STATUS_BADGE_CLASS[instance.status]}>{STATUS_LABEL[instance.status]}</Badge>
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
          <TabsTrigger value="players">Players</TabsTrigger>
          <TabsTrigger value="configuration">Configuration</TabsTrigger>
          <TabsTrigger value="diagnostics">Diagnostics</TabsTrigger>
          <TabsTrigger value="logs">Logs</TabsTrigger>
        </TabsList>

        <TabsContent value="overview" className="flex flex-col gap-4">
          <ForgeInstallBanner instance={instance} />
          {instance.status === "running" && <InstanceDashboard instance={instance} />}
          {/* Unconditional, unlike the live dashboard above: recorded history
              is most useful precisely when the server is stopped. */}
          <PerformanceHistoryCard instance={instance} />
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
          <InstanceFilesTab instance={instance} />
        </TabsContent>
        <TabsContent value="mods">
          <InstanceModsTab instance={instance} />
        </TabsContent>
        <TabsContent value="players" className="flex flex-col gap-4">
          <OnlinePlayersCard instance={instance} />
          <InstancePlayersTab instanceId={instance.id} />
        </TabsContent>
        <TabsContent value="configuration" className="flex flex-col gap-4">
          <ModpackUpdatesCard instance={instance} />
          <AutomationCard instance={instance} />
          <ServerPropertiesForm instance={instance} />
          <InstanceSettingsForm instance={instance} />
        </TabsContent>
        <TabsContent value="diagnostics">
          <DiagnosticsTab instance={instance} />
        </TabsContent>
        <TabsContent value="logs">
          <InstanceLogsTab instance={instance} />
        </TabsContent>
      </Tabs>
    </div>
  );
}
