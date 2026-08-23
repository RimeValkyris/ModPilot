import { useEffect, useState } from "react";
import { toast } from "sonner";
import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { openPath } from "@tauri-apps/plugin-opener";
import {
  disable as disableAutostart,
  enable as enableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart";
import { getVersion } from "@tauri-apps/api/app";
import { AlertTriangle, Check, Download, FolderOpen } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { api } from "@/lib/tauri";
import { useThemeStore } from "@/stores/themeStore";
import { useAppSetting, useBoolAppSetting } from "@/hooks/useAppSetting";
import { THEMES, THEME_LABELS, THEME_SWATCHES } from "@/types/theme";

function SettingsSection({
  title,
  description,
  children,
}: {
  title: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <section className="flex flex-col gap-4 rounded-xl border border-border bg-card p-4">
      <div>
        <h2 className="text-sm font-medium">{title}</h2>
        {description && <p className="text-xs text-muted-foreground">{description}</p>}
      </div>
      {children}
    </section>
  );
}

function SettingRow({
  label,
  description,
  children,
}: {
  label: string;
  description?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-4">
      <div>
        <Label>{label}</Label>
        {description && <p className="text-xs text-muted-foreground">{description}</p>}
      </div>
      {children}
    </div>
  );
}

function AppearanceSection() {
  const { theme, setTheme } = useThemeStore();

  return (
    <SettingsSection title="Appearance" description="Theme for the whole app.">
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
        {THEMES.map((t) => (
          <button
            key={t}
            type="button"
            onClick={() => setTheme(t)}
            className={`flex flex-col items-center gap-2 rounded-lg border p-3 text-xs transition-colors ${
              theme === t ? "border-primary" : "border-border hover:border-foreground/30"
            }`}
          >
            <span
              className="flex size-10 items-center justify-center rounded-full border border-border"
              style={{ backgroundColor: THEME_SWATCHES[t].background }}
            >
              {theme === t && (
                <Check className="size-4" style={{ color: THEME_SWATCHES[t].accent }} />
              )}
            </span>
            {THEME_LABELS[t]}
          </button>
        ))}
      </div>
    </SettingsSection>
  );
}

function BehaviorSection() {
  const { value: minimizeToTray, setValue: setMinimizeToTray } = useBoolAppSetting(
    "minimize_to_tray",
    false,
  );
  const { value: notificationsEnabled, setValue: setNotificationsEnabled } = useBoolAppSetting(
    "notifications_enabled",
    true,
  );
  const [autostart, setAutostart] = useState(false);
  const [isAutostartLoaded, setIsAutostartLoaded] = useState(false);

  useEffect(() => {
    isAutostartEnabled()
      .then(setAutostart)
      .finally(() => setIsAutostartLoaded(true));
  }, []);

  async function handleAutostartChange(next: boolean) {
    setAutostart(next);
    try {
      if (next) await enableAutostart();
      else await disableAutostart();
    } catch (err) {
      setAutostart(!next);
      toast.error("Failed to change startup setting", { description: String(err) });
    }
  }

  return (
    <SettingsSection title="Behavior">
      <SettingRow
        label="Launch ModpackPilot on system startup"
        description="Start ModpackPilot automatically when you sign in to Windows."
      >
        <Switch
          checked={autostart}
          disabled={!isAutostartLoaded}
          onCheckedChange={handleAutostartChange}
        />
      </SettingRow>
      <SettingRow
        label="Minimize to tray instead of closing"
        description="Closing the window hides it to the system tray; servers keep running. Use Quit from the tray menu to actually exit."
      >
        <Switch checked={minimizeToTray} onCheckedChange={setMinimizeToTray} />
      </SettingRow>
      <SettingRow
        label="Desktop notifications"
        description="Notify when a server finishes starting or crashes."
      >
        <Switch checked={notificationsEnabled} onCheckedChange={setNotificationsEnabled} />
      </SettingRow>
    </SettingsSection>
  );
}

function InstancesLocationSection() {
  const [currentDir, setCurrentDir] = useState("");
  const [chosenDir, setChosenDir] = useState<string | null>(null);
  const [moveExisting, setMoveExisting] = useState(true);
  const [isSaving, setIsSaving] = useState(false);

  useEffect(() => {
    api.getInstancesDir().then(setCurrentDir).catch(() => {});
  }, []);

  async function handleChoose() {
    const dir = await openDialog({ title: "Choose instances folder", directory: true });
    if (typeof dir === "string") setChosenDir(dir);
  }

  async function handleSave() {
    if (!chosenDir) return;
    setIsSaving(true);
    try {
      await api.setInstancesDir(chosenDir, moveExisting);
      toast.success("Saved. Restart ModpackPilot to switch to the new location.");
      setCurrentDir(chosenDir);
      setChosenDir(null);
    } catch (err) {
      toast.error("Failed to change instances location", { description: String(err) });
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <SettingsSection
      title="Instances Location"
      description="Where new and existing server instances are stored."
    >
      <p className="truncate rounded-lg border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
        {currentDir || "Loading…"}
      </p>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="outline" size="sm" onClick={handleChoose}>
          <FolderOpen />
          Choose Folder
        </Button>
        {chosenDir && (
          <>
            <span className="text-xs text-muted-foreground">→ {chosenDir}</span>
          </>
        )}
      </div>
      {chosenDir && (
        <div className="flex flex-col gap-2 border-t border-border pt-3">
          <label className="flex items-center gap-2 text-xs text-muted-foreground">
            <input
              type="checkbox"
              checked={moveExisting}
              onChange={(e) => setMoveExisting(e.target.checked)}
            />
            Move existing instances to the new folder
          </label>
          <Button size="sm" className="w-fit" disabled={isSaving} onClick={handleSave}>
            {isSaving ? "Saving…" : "Save (requires restart)"}
          </Button>
        </div>
      )}
    </SettingsSection>
  );
}

function NewInstanceDefaultsSection() {
  const { value: minRam, setValue: setMinRam } = useAppSetting("default_min_ram_mb", "2048");
  const { value: maxRam, setValue: setMaxRam } = useAppSetting("default_max_ram_mb", "4096");
  const { value: jvmArgs, setValue: setJvmArgs } = useAppSetting("default_jvm_args", "");

  return (
    <SettingsSection
      title="New Instance Defaults"
      description="Pre-filled values whenever you create a new instance."
    >
      <div className="grid grid-cols-2 gap-3">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="default-min-ram">Default Min RAM (MB)</Label>
          <Input
            id="default-min-ram"
            type="number"
            min={512}
            step={512}
            value={minRam}
            onChange={(e) => setMinRam(e.target.value)}
          />
        </div>
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="default-max-ram">Default Max RAM (MB)</Label>
          <Input
            id="default-max-ram"
            type="number"
            min={512}
            step={512}
            value={maxRam}
            onChange={(e) => setMaxRam(e.target.value)}
          />
        </div>
      </div>
      <div className="flex flex-col gap-1.5">
        <Label htmlFor="default-jvm-args">
          Default JVM arguments <span className="text-muted-foreground">(one per line)</span>
        </Label>
        <Textarea
          id="default-jvm-args"
          rows={3}
          className="font-mono text-xs"
          placeholder="-XX:+UseG1GC"
          value={jvmArgs}
          onChange={(e) => setJvmArgs(e.target.value)}
        />
        <p className="text-xs text-muted-foreground">
          Applied to newly created instances only (not imports, which detect their own).
        </p>
      </div>
    </SettingsSection>
  );
}

function ConsoleSection() {
  const { value: maxLines, setValue: setMaxLines } = useAppSetting("console_max_lines", "2000");
  const { value: fontSize, setValue: setFontSize } = useAppSetting("console_font_size", "xs");
  const { value: wordWrap, setValue: setWordWrap } = useBoolAppSetting("console_word_wrap", false);

  return (
    <SettingsSection
      title="Console"
      description="Controls the live console's performance and readability."
    >
      <div className="grid grid-cols-2 gap-3">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="console-max-lines">Max retained lines</Label>
          <Input
            id="console-max-lines"
            type="number"
            min={100}
            step={100}
            value={maxLines}
            onChange={(e) => setMaxLines(e.target.value)}
          />
          <p className="text-xs text-muted-foreground">Lower this if the console feels slow.</p>
        </div>
        <div className="flex flex-col gap-1.5">
          <Label>Font size</Label>
          <Select value={fontSize} onValueChange={(v) => v && setFontSize(v)}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="xs">Small</SelectItem>
              <SelectItem value="sm">Medium</SelectItem>
              <SelectItem value="base">Large</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>
      <SettingRow label="Wrap long lines" description="Off by default to keep log alignment readable.">
        <Switch checked={wordWrap} onCheckedChange={setWordWrap} />
      </SettingRow>
    </SettingsSection>
  );
}

function DiagnosticsSection() {
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
      title: "Save ModpackPilot log",
      defaultPath: `modpackpilot-log-${new Date().toISOString().slice(0, 10)}.txt`,
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
    <SettingsSection
      title="Diagnostics"
      description="If ModpackPilot crashes or behaves unexpectedly, export the log and include it when reporting the issue."
    >
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
    </SettingsSection>
  );
}

function AboutSection() {
  const [version, setVersion] = useState("");

  useEffect(() => {
    getVersion()
      .then(setVersion)
      .catch(() => {});
  }, []);

  return (
    <SettingsSection title="About">
      <p className="text-sm">
        ModpackPilot <span className="text-muted-foreground">v{version || "…"}</span>
      </p>
      <p className="text-xs text-muted-foreground">
        A native desktop launcher and manager for Minecraft modpack servers.
      </p>
    </SettingsSection>
  );
}

function DangerZoneSection() {
  const [confirmOpen, setConfirmOpen] = useState(false);
  const [isResetting, setIsResetting] = useState(false);

  async function handleReset() {
    setIsResetting(true);
    try {
      await api.resetJavaInstallations();
      toast.success("Java installations forgotten. Rescan on the Java page to detect them again.");
      setConfirmOpen(false);
    } catch (err) {
      toast.error("Failed to reset Java installations", { description: String(err) });
    } finally {
      setIsResetting(false);
    }
  }

  return (
    <SettingsSection title="Danger Zone">
      <SettingRow
        label="Forget all Java installations"
        description="Clears detected Java entries (not any instance's other settings). Rescan afterward to detect them again."
      >
        <Button variant="destructive" size="sm" onClick={() => setConfirmOpen(true)}>
          <AlertTriangle />
          Reset
        </Button>
      </SettingRow>

      <AlertDialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>Forget all Java installations?</AlertDialogTitle>
            <AlertDialogDescription>
              Any instance currently assigned one of these will have its Java
              selection cleared. This doesn't uninstall Java or touch any
              instance's files - you can rescan afterward.
            </AlertDialogDescription>
          </AlertDialogHeader>
          <AlertDialogFooter>
            <AlertDialogCancel>Cancel</AlertDialogCancel>
            <AlertDialogAction
              className="bg-destructive text-white hover:bg-destructive/90"
              disabled={isResetting}
              onClick={handleReset}
            >
              {isResetting ? "Resetting…" : "Reset"}
            </AlertDialogAction>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    </SettingsSection>
  );
}

export function SettingsPage() {
  return (
    <div className="mx-auto flex max-w-5xl flex-col gap-6 p-8">
      <header>
        <h1 className="text-2xl font-semibold tracking-tight">Settings</h1>
        <p className="text-sm text-muted-foreground">Application-level settings and diagnostics.</p>
      </header>

      <AppearanceSection />
      <BehaviorSection />
      <InstancesLocationSection />
      <NewInstanceDefaultsSection />
      <ConsoleSection />
      <DiagnosticsSection />
      <AboutSection />
      <DangerZoneSection />
    </div>
  );
}
