import { useEffect, useState } from "react";
import { toast } from "sonner";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ImagePlus, Trash2 } from "lucide-react";
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
import { useInstances } from "@/hooks/useInstances";
import { useWallpaperStore } from "@/stores/wallpaperStore";
import { api } from "@/lib/tauri";
import type { Instance } from "@/types/instance";

const NO_JAR_VALUE = "__none__";

function linesToArgs(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

export function InstanceSettingsForm({ instance }: { instance: Instance }) {
  const { updateInstanceSettings } = useInstances();
  const { wallpapers, fetchWallpaper, setWallpaper, clearWallpaper } = useWallpaperStore();
  const wallpaper = wallpapers[instance.id];

  const [jars, setJars] = useState<string[]>([]);
  const [serverJar, setServerJar] = useState(instance.serverJar ?? NO_JAR_VALUE);
  const [jvmArgs, setJvmArgs] = useState(instance.jvmArgs.join("\n"));
  const [serverArgs, setServerArgs] = useState(instance.serverArgs.join("\n"));
  const [minRamMb, setMinRamMb] = useState(String(instance.minRamMb));
  const [maxRamMb, setMaxRamMb] = useState(String(instance.maxRamMb));
  const [autoStart, setAutoStart] = useState(instance.autoStart);
  const [autoRestart, setAutoRestart] = useState(instance.autoRestart);
  const [isSaving, setIsSaving] = useState(false);
  const [isWallpaperBusy, setIsWallpaperBusy] = useState(false);

  useEffect(() => {
    fetchWallpaper(instance.id);
    api
      .listServerJars(instance.id)
      .then(setJars)
      .catch(() => setJars([]));
  }, [instance.id, fetchWallpaper]);

  async function handleSave(e: React.FormEvent) {
    e.preventDefault();
    const min = Number(minRamMb);
    const max = Number(maxRamMb);
    if (!min || !max || min > max) {
      toast.error("Minimum RAM must be positive and not exceed maximum RAM");
      return;
    }

    setIsSaving(true);
    try {
      await updateInstanceSettings(instance.id, {
        serverJar: serverJar === NO_JAR_VALUE ? null : serverJar,
        jvmArgs: linesToArgs(jvmArgs),
        serverArgs: linesToArgs(serverArgs),
        minRamMb: min,
        maxRamMb: max,
        autoStart,
        autoRestart,
      });
      toast.success("Settings saved");
    } catch (err) {
      toast.error("Failed to save settings", { description: String(err) });
    } finally {
      setIsSaving(false);
    }
  }

  async function handlePickWallpaper() {
    const path = await openDialog({
      title: "Choose a wallpaper image",
      multiple: false,
      directory: false,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
    });
    if (typeof path !== "string") return;

    setIsWallpaperBusy(true);
    try {
      await setWallpaper(instance.id, path);
    } catch (err) {
      toast.error("Failed to set wallpaper", { description: String(err) });
    } finally {
      setIsWallpaperBusy(false);
    }
  }

  async function handleClearWallpaper() {
    setIsWallpaperBusy(true);
    try {
      await clearWallpaper(instance.id);
    } catch (err) {
      toast.error("Failed to remove wallpaper", { description: String(err) });
    } finally {
      setIsWallpaperBusy(false);
    }
  }

  return (
    <div className="flex flex-col gap-6">
      <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
        <h2 className="text-sm font-medium">Wallpaper</h2>
        {wallpaper ? (
          <img
            src={wallpaper}
            alt=""
            className="h-32 w-full rounded-lg object-cover"
          />
        ) : (
          <div className="flex h-32 w-full items-center justify-center rounded-lg border border-dashed border-border text-sm text-muted-foreground">
            No wallpaper set
          </div>
        )}
        <div className="flex gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={isWallpaperBusy}
            onClick={handlePickWallpaper}
          >
            <ImagePlus />
            Choose Image
          </Button>
          {wallpaper && (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={isWallpaperBusy}
              onClick={handleClearWallpaper}
            >
              <Trash2 />
              Remove
            </Button>
          )}
        </div>
      </section>

      <form onSubmit={handleSave} className="flex flex-col gap-4 rounded-xl border border-border bg-card p-4">
        <h2 className="text-sm font-medium">Launch Settings</h2>

        <div className="flex flex-col gap-1.5">
          <Label>Server JAR</Label>
          <Select value={serverJar} onValueChange={(v) => v && setServerJar(v)}>
            <SelectTrigger className="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value={NO_JAR_VALUE}>Not set</SelectItem>
              {jars.map((jar) => (
                <SelectItem key={jar} value={jar}>
                  {jar}
                </SelectItem>
              ))}
            </SelectContent>
          </Select>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="settings-min-ram">Min RAM (MB)</Label>
            <Input
              id="settings-min-ram"
              type="number"
              min={512}
              step={512}
              value={minRamMb}
              onChange={(e) => setMinRamMb(e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="settings-max-ram">Max RAM (MB)</Label>
            <Input
              id="settings-max-ram"
              type="number"
              min={512}
              step={512}
              value={maxRamMb}
              onChange={(e) => setMaxRamMb(e.target.value)}
            />
          </div>
        </div>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="settings-jvm-args">
            JVM arguments <span className="text-muted-foreground">(one per line)</span>
          </Label>
          <Textarea
            id="settings-jvm-args"
            rows={4}
            className="font-mono text-xs"
            placeholder={`-Xms${instance.minRamMb}M\n-Xmx${instance.maxRamMb}M\n-XX:+UseG1GC`}
            value={jvmArgs}
            onChange={(e) => setJvmArgs(e.target.value)}
          />
          <p className="text-xs text-muted-foreground">
            Leave empty to auto-generate from the RAM settings above.
          </p>
        </div>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="settings-server-args">
            Server arguments <span className="text-muted-foreground">(one per line)</span>
          </Label>
          <Textarea
            id="settings-server-args"
            rows={2}
            className="font-mono text-xs"
            placeholder="nogui"
            value={serverArgs}
            onChange={(e) => setServerArgs(e.target.value)}
          />
        </div>

        <div className="flex items-center justify-between">
          <div>
            <Label htmlFor="settings-auto-start">Auto-start</Label>
            <p className="text-xs text-muted-foreground">
              Launch this instance when ModForge starts.
            </p>
          </div>
          <Switch id="settings-auto-start" checked={autoStart} onCheckedChange={setAutoStart} />
        </div>

        <div className="flex items-center justify-between">
          <div>
            <Label htmlFor="settings-auto-restart">Auto-restart</Label>
            <p className="text-xs text-muted-foreground">
              Restart automatically if the server crashes.
            </p>
          </div>
          <Switch
            id="settings-auto-restart"
            checked={autoRestart}
            onCheckedChange={setAutoRestart}
          />
        </div>

        <Button type="submit" disabled={isSaving} className="w-fit">
          {isSaving ? "Saving…" : "Save Settings"}
        </Button>
      </form>
    </div>
  );
}
