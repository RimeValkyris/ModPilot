import { Server as ServerIcon } from "lucide-react";
import { useInstances } from "@/hooks/useInstances";
import { CreateInstanceDialog } from "@/features/dashboard/CreateInstanceDialog";
import { ImportServerDialog } from "@/features/dashboard/ImportServerDialog";
import { InstanceCard } from "@/features/dashboard/InstanceCard";

/**
 * The actual server list + create/import actions. Shared by the Dashboard
 * and Servers nav pages, which currently show the same content - kept as a
 * single component so instance-list logic isn't duplicated between them.
 */
export function ServerInstancesView({
  title,
  subtitle,
}: {
  title: string;
  subtitle: string;
}) {
  const { instances, isLoading, error } = useInstances();

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 p-8">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">{title}</h1>
        <p className="text-sm text-muted-foreground">{subtitle}</p>
      </header>

      {error && (
        <div className="rounded-lg border border-destructive/30 bg-destructive/10 px-4 py-3 text-sm text-destructive">
          Failed to load instances: {error}
        </div>
      )}

      {isLoading ? (
        <div className="rounded-xl border border-border bg-card px-6 py-16 text-center text-sm text-muted-foreground">
          Loading instances…
        </div>
      ) : instances.length === 0 ? (
        <div className="flex flex-col items-center gap-3 rounded-xl border border-dashed border-border bg-card px-6 py-16 text-center">
          <ServerIcon className="size-8 text-muted-foreground" />
          <p className="text-base font-medium">No servers installed</p>
          <p className="max-w-sm text-sm text-muted-foreground">
            Import an existing Minecraft server, or create a new one, to get
            started.
          </p>
        </div>
      ) : (
        <ul className="grid grid-cols-2 gap-4 sm:grid-cols-3 lg:grid-cols-4">
          {instances.map((instance) => (
            <InstanceCard key={instance.id} instance={instance} />
          ))}
        </ul>
      )}

      <div className="flex gap-3">
        <ImportServerDialog />
        <CreateInstanceDialog />
      </div>
    </div>
  );
}
