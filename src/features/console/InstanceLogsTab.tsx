import { useState } from "react";
import { toast } from "sonner";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { Download, FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/tauri";
import type { Instance } from "@/types/instance";

export function InstanceLogsTab({ instance }: { instance: Instance }) {
  const [isExporting, setIsExporting] = useState(false);

  async function handleOpenFolder() {
    try {
      const dir = await api.getInstanceLogsDir(instance.id);
      await openPath(dir);
    } catch (err) {
      toast.error("Failed to open logs folder", { description: String(err) });
    }
  }

  async function handleExport() {
    const destPath = await saveDialog({
      title: "Save server log",
      defaultPath: `${instance.name}-log-${new Date().toISOString().slice(0, 10)}.txt`,
      filters: [{ name: "Log file", extensions: ["txt", "log"] }],
    });
    if (!destPath) return;

    setIsExporting(true);
    try {
      await api.exportInstanceLog(instance.id, destPath);
      toast.success("Log exported");
    } catch (err) {
      toast.error("Failed to export log", { description: String(err) });
    } finally {
      setIsExporting(false);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <div className="rounded-xl border border-border bg-card p-4 text-sm text-muted-foreground">
        {instance.status === "crashed"
          ? "This instance crashed. Export its log below to inspect what happened or attach it to a bug report."
          : "Logs are written to this instance's own logs/ folder as it runs."}
      </div>
      <div className="flex gap-2">
        <Button variant="outline" size="sm" onClick={handleOpenFolder}>
          <FolderOpen />
          Open Logs Folder
        </Button>
        <Button variant="outline" size="sm" disabled={isExporting} onClick={handleExport}>
          <Download />
          {isExporting ? "Exporting…" : "Export Log"}
        </Button>
      </div>
    </div>
  );
}
