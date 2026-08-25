import { useEffect } from "react";
import { Link } from "react-router-dom";
import {
  AlertTriangle,
  ArrowRight,
  Cpu,
  MemoryStick,
  Server as ServerIcon,
} from "lucide-react";
import { useInstances } from "@/hooks/useInstances";
import { useResourceUsagePolling } from "@/hooks/useResourceUsagePolling";
import { useResourceUsageStore } from "@/stores/resourceUsageStore";
import { useJavaStore } from "@/stores/javaStore";
import { getRequiredJavaMajor, parseJavaMajor } from "@/lib/javaRequirement";
import { formatMemoryMb } from "@/lib/format";
import { STATUS_DOT } from "@/lib/serverStatus";
import { CreateInstanceDialog } from "@/features/dashboard/CreateInstanceDialog";
import { ImportServerDialog } from "@/features/dashboard/ImportServerDialog";
import { InstanceCard } from "@/features/dashboard/InstanceCard";
import { ResourceTrends } from "@/features/dashboard/ResourceTrends";
import type { Instance } from "@/types/instance";

function StatCard({
  icon: Icon,
  label,
  value,
  detail,
  tint = "bg-primary/10 text-primary",
}: {
  icon: React.ComponentType<{ className?: string }>;
  label: string;
  value: string;
  detail?: string;
  tint?: string;
}) {
  return (
    <div className="flex items-center gap-3 rounded-xl border border-border bg-card p-4 shadow-sm">
      <div className={`flex size-10 shrink-0 items-center justify-center rounded-lg ${tint}`}>
        <Icon className="size-4.5" />
      </div>
      <div>
        <p className="text-lg font-semibold leading-none">{value}</p>
        <p className="mt-1 text-xs text-muted-foreground">{label}{detail ? ` · ${detail}` : ""}</p>
      </div>
    </div>
  );
}

export function Dashboard() {
  const { instances, isLoading } = useInstances();
  useResourceUsagePolling();
  const usageByInstanceId = useResourceUsageStore((s) => s.usageByInstanceId);
  const { installations: javaInstallations, fetchInstallations } = useJavaStore();

  useEffect(() => {
    fetchInstallations();
  }, [fetchInstallations]);

  const running = instances.filter((i) => i.status === "running");
  const crashed = instances.filter((i) => i.status === "crashed");

  const javaMismatched = instances.filter((i) => {
    const required = getRequiredJavaMajor(i.minecraftVersion);
    const assigned = javaInstallations.find((j) => j.id === i.javaInstallationId);
    const assignedMajor = assigned ? parseJavaMajor(assigned.version) : null;
    return required !== null && assignedMajor !== null && assignedMajor !== required;
  });

  const needsAttention = [...crashed, ...javaMismatched.filter((i) => i.status !== "crashed")];

  const totalCpu = running.reduce((sum, i) => sum + (usageByInstanceId[i.id]?.cpuPercent ?? 0), 0);
  const totalRamUsed = running.reduce((sum, i) => sum + (usageByInstanceId[i.id]?.memoryMb ?? 0), 0);
  const totalRamAllocated = running.reduce((sum, i) => sum + i.maxRamMb, 0);

  const recent = [...instances]
    .sort((a, b) => {
      const aTime = a.lastLaunchedAt ?? a.createdAt;
      const bTime = b.lastLaunchedAt ?? b.createdAt;
      return new Date(bTime).getTime() - new Date(aTime).getTime();
    })
    .slice(0, 4);

  if (isLoading && instances.length === 0) {
    return <div className="p-8 text-sm text-muted-foreground">Loading…</div>;
  }

  if (instances.length === 0) {
    return (
      <div className="mx-auto flex max-w-5xl flex-col gap-6 p-8">
        <header>
          <h1 className="text-2xl font-semibold tracking-tight">Dashboard</h1>
          <p className="text-sm text-muted-foreground">
            An overview of your Minecraft modpack servers.
          </p>
        </header>
        <div className="flex flex-col items-center gap-3 rounded-xl border border-dashed border-border bg-card px-6 py-16 text-center">
          <ServerIcon className="size-8 text-muted-foreground" />
          <p className="text-base font-medium">No servers installed</p>
          <p className="max-w-sm text-sm text-muted-foreground">
            Import an existing Minecraft server, or create a new one, to get started.
          </p>
          <div className="mt-2 flex gap-3">
            <ImportServerDialog />
            <CreateInstanceDialog />
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 p-8">
      <header className="flex items-center justify-between gap-4">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Dashboard</h1>
          <p className="text-sm text-muted-foreground">
            An overview of your Minecraft modpack servers.
          </p>
        </div>
        <div className="flex gap-2">
          <ImportServerDialog />
          <CreateInstanceDialog />
        </div>
      </header>

      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        <StatCard
          icon={ServerIcon}
          label="Servers"
          value={String(instances.length)}
          detail={`${running.length} running`}
          tint="bg-primary/10 text-primary"
        />
        <StatCard
          icon={AlertTriangle}
          label="Needs attention"
          value={String(needsAttention.length)}
          tint={
            needsAttention.length > 0
              ? "bg-destructive/10 text-destructive"
              : "bg-muted text-muted-foreground"
          }
        />
        <StatCard
          icon={Cpu}
          label="CPU in use"
          value={running.length > 0 ? `${totalCpu.toFixed(0)}%` : "—"}
          tint="bg-blue-500/10 text-blue-600 dark:text-blue-400"
        />
        <StatCard
          icon={MemoryStick}
          label="RAM in use"
          value={running.length > 0 ? formatMemoryMb(totalRamUsed) : "—"}
          detail={running.length > 0 ? `of ${formatMemoryMb(totalRamAllocated)}` : undefined}
          tint="bg-violet-500/10 text-violet-600 dark:text-violet-400"
        />
      </div>

      {needsAttention.length > 0 && (
        <section className="flex flex-col gap-2 rounded-xl border border-destructive/30 bg-destructive/5 p-4">
          <h2 className="flex items-center gap-1.5 text-sm font-medium text-destructive">
            <AlertTriangle className="size-4" />
            Needs attention
          </h2>
          <ul className="flex flex-col gap-1.5">
            {needsAttention.map((instance: Instance) => (
              <li key={instance.id}>
                <Link
                  to={`/instances/${instance.id}`}
                  className="flex items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-sm hover:bg-destructive/10"
                >
                  <span className="flex items-center gap-2">
                    <span className={`size-2 rounded-full ${STATUS_DOT[instance.status]}`} />
                    {instance.name}
                  </span>
                  <span className="text-xs text-muted-foreground">
                    {instance.status === "crashed" ? "Crashed" : "Java version mismatch"}
                  </span>
                </Link>
              </li>
            ))}
          </ul>
        </section>
      )}

      <ResourceTrends instances={instances} />

      <section className="flex flex-col gap-3">
        <div className="flex items-center justify-between">
          <h2 className="text-sm font-medium text-muted-foreground">Recent Servers</h2>
          <Link
            to="/servers"
            className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
          >
            View all servers
            <ArrowRight className="size-3" />
          </Link>
        </div>
        <ul className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">
          {recent.map((instance) => (
            <InstanceCard key={instance.id} instance={instance} />
          ))}
        </ul>
      </section>
    </div>
  );
}
