import { useEffect, useState } from "react";
import { toast } from "sonner";
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
import { useInstancesStore } from "@/stores/instancesStore";
import { api } from "@/lib/tauri";
import type { Instance } from "@/types/instance";

const NO_JAR_VALUE = "__none__";

/** Matches Rust's `Instance::launch_mode` for a pack run through its own
 * `run.bat`/`run.sh`, where the script - not ModpackPilot - supplies the
 * JVM and program arguments. */
function isScriptMode(instance: Instance, selectedTarget: string): boolean {
  if (/\.(bat|cmd|sh)$/i.test(selectedTarget)) return true;
  return instance.launchMode === "script" && selectedTarget === instance.serverJar;
}

function linesToArgs(text: string): string[] {
  return text
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line.length > 0);
}

export function InstanceSettingsForm({ instance }: { instance: Instance }) {
  const { updateInstanceSettings, redetectInstanceLaunch } = useInstancesStore();

  const [jars, setJars] = useState<string[]>([]);
  const [serverJar, setServerJar] = useState(instance.serverJar ?? NO_JAR_VALUE);
  const [jvmArgs, setJvmArgs] = useState(instance.jvmArgs.join("\n"));
  const [serverArgs, setServerArgs] = useState(instance.serverArgs.join("\n"));
  const [minRamMb, setMinRamMb] = useState(String(instance.minRamMb));
  const [maxRamMb, setMaxRamMb] = useState(String(instance.maxRamMb));
  const [systemMemoryMb, setSystemMemoryMb] = useState<number | null>(null);

  useEffect(() => {
    api.getSystemMemoryMb().then(setSystemMemoryMb).catch(() => {});
  }, []);
  const [isRedetecting, setIsRedetecting] = useState(false);
  const [autoStart, setAutoStart] = useState(instance.autoStart);
  const [autoRestart, setAutoRestart] = useState(instance.autoRestart);
  const [isSaving, setIsSaving] = useState(false);

  useEffect(() => {
    api
      .listServerJars(instance.id)
      .then(setJars)
      .catch(() => setJars([]));
  }, [instance.id, instance.serverJar]);

  const scriptMode = isScriptMode(instance, serverJar);

  async function handleRedetect() {
    setIsRedetecting(true);
    try {
      const updated = await redetectInstanceLaunch(instance.id);
      setServerJar(updated.serverJar ?? NO_JAR_VALUE);
      toast.success(`Detected launch target: ${updated.serverJar}`);
    } catch (err) {
      toast.error("Couldn't detect anything launchable", { description: String(err) });
    } finally {
      setIsRedetecting(false);
    }
  }

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

  return (
    <form onSubmit={handleSave} className="flex flex-col gap-4 rounded-xl border border-border bg-card p-4">
      <h2 className="text-sm font-medium">Launch Settings</h2>

      <div className="flex flex-col gap-1.5">
        <Label>Launch target</Label>
        <div className="flex gap-2">
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
          <Button
            type="button"
            variant="outline"
            onClick={handleRedetect}
            disabled={isRedetecting}
            className="shrink-0"
          >
            {isRedetecting ? "Scanning…" : "Re-scan"}
          </Button>
        </div>
        <p className="text-xs text-muted-foreground">
          The server JAR, Forge/NeoForge argfile, or start script this instance launches.
          Re-scan reads the server folder again - useful if the pack was repaired, installed,
          or imported before ModpackPilot could recognize it.
        </p>
      </div>

      {scriptMode && (
        <p className="rounded-lg border border-border bg-muted/30 px-3 py-2 text-xs text-muted-foreground">
          This instance starts through the pack's own <code>{serverJar}</code>, which
          supplies its own JVM and server arguments - the two argument boxes below are not used.
          RAM is still applied when the pack ships a <code>user_jvm_args.txt</code> (Forge and
          NeoForge do); otherwise edit the script itself.
        </p>
      )}

      <div className="grid grid-cols-2 gap-3">
        <div className="flex flex-col gap-1.5">
          <Label htmlFor="settings-min-ram">Min RAM (MB)</Label>
          <Input
            id="settings-min-ram"
            type="number"
            min={512}
            step={1}
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
            step={1}
            value={maxRamMb}
            onChange={(e) => setMaxRamMb(e.target.value)}
          />
        </div>
      </div>
      {systemMemoryMb !== null && (
        <p className="text-xs text-muted-foreground">
          This machine has {(systemMemoryMb / 1024).toFixed(1)} GB ({systemMemoryMb.toLocaleString()}{" "}
          MB) of RAM installed. Leave enough headroom for the OS and any other running
          servers - don't allocate all of it to one instance.
        </p>
      )}

      <div className="flex flex-col gap-1.5">
        <Label htmlFor="settings-jvm-args">
          JVM arguments <span className="text-muted-foreground">(one per line)</span>
        </Label>
        <Textarea
          id="settings-jvm-args"
          disabled={scriptMode}
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
          disabled={scriptMode}
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
            Launch this instance when ModpackPilot starts.
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
  );
}
