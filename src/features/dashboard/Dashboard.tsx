import { ServerInstancesView } from "@/features/dashboard/ServerInstancesView";

export function Dashboard() {
  return (
    <ServerInstancesView
      title="My Servers"
      subtitle="Manage your Minecraft modpack server instances."
    />
  );
}
