import { toast } from "sonner";
import { PackagePlus, Server as ServerIcon } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useInstances } from "@/hooks/useInstances";
import { CreateInstanceDialog } from "@/features/dashboard/CreateInstanceDialog";
import { InstanceCard } from "@/features/dashboard/InstanceCard";

function notImplemented(feature: string) {
  toast.info(`${feature} isn't implemented yet`, {
    description: "This will be wired up in an upcoming phase.",
  });
}

export function Dashboard() {
  const { instances, isLoading, error } = useInstances();

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 p-8">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">My Servers</h1>
        <p className="text-sm text-muted-foreground">
          Manage your Minecraft modpack server instances.
        </p>
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
        <ul className="grid grid-cols-1 gap-4 sm:grid-cols-2">
          {instances.map((instance) => (
            <InstanceCard key={instance.id} instance={instance} />
          ))}
        </ul>
      )}

      <div className="flex gap-3">
        <Button variant="outline" onClick={() => notImplemented("Import Server")}>
          <PackagePlus />
          Import Server
        </Button>
        <CreateInstanceDialog />
      </div>
    </div>
  );
}
