import { useState } from "react";
import { toast } from "sonner";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import { FolderOpen, Download } from "lucide-react";
import { Button } from "@/components/ui/button";
import { api } from "@/lib/tauri";

export function SettingsPage() {
  const [isExporting, setIsExporting] = useState(false);

  async function handleOpenLogsFolder() {
    try {
      const dir = await api.getAppLogsDir();
      await openPath(dir);
    } catch (err) {
      toast.error("Failed to open logs folder", { description: String(err) });
    }
  }

  async function handleExportLog() {
    const destPath = await saveDialog({
      title: "Save ModForge log",
      defaultPath: `modforge-log-${new Date().toISOString().slice(0, 10)}.txt`,
      filters: [{ name: "Log file", extensions: ["txt", "log"] }],
    });
    if (!destPath) return;

    setIsExporting(true);
    try {
      await api.exportAppLog(destPath);
      toast.success("Log exported");
    } catch (err) {
      toast.error("Failed to export log", { description: String(err) });
    } finally {
      setIsExporting(false);
    }
  }

  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 p-8">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">Application-level settings and diagnostics.</p>
      </header>

      <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
        <div>
          <h2 className="text-sm font-medium">Diagnostics</h2>
          <p className="text-xs text-muted-foreground">
            If ModForge crashes or behaves unexpectedly, export the log and
            include it when reporting the issue.
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={handleOpenLogsFolder}>
            <FolderOpen />
            Open Logs Folder
          </Button>
          <Button variant="outline" size="sm" disabled={isExporting} onClick={handleExportLog}>
            <Download />
            {isExporting ? "Exporting…" : "Export Crash Log"}
          </Button>
        </div>
      </section>
    </div>
  );
}
