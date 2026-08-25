import { useEffect, useState } from "react";
import { toast } from "sonner";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { ImagePlus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { api } from "@/lib/tauri";
import { ServerAvatarPlaceholder } from "@/components/ServerAvatarPlaceholder";
import type { Instance } from "@/types/instance";
import type { AvatarPresetInfo } from "@/types/avatar";

/** Vanilla's own defaults - shown until server.properties exists to read from. */
const DEFAULTS: Record<string, string> = {
  motd: "A Minecraft Server",
  "max-players": "20",
  difficulty: "easy",
  gamemode: "survival",
  pvp: "true",
  "online-mode": "true",
  "white-list": "false",
  hardcore: "false",
  "view-distance": "10",
  "spawn-protection": "16",
};

export function ServerPropertiesForm({ instance }: { instance: Instance }) {
  const [values, setValues] = useState<Record<string, string>>(DEFAULTS);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);

  const [icon, setIcon] = useState<string | null>(null);
  const [isIconBusy, setIsIconBusy] = useState(false);

  const [avatar, setAvatar] = useState<string | null>(null);
  const [isAvatarBusy, setIsAvatarBusy] = useState(false);
  const [avatarPresets, setAvatarPresets] = useState<AvatarPresetInfo[]>([]);

  useEffect(() => {
    setIsLoading(true);
    Promise.all([
      api.readServerProperties(instance.id),
      api.readServerIcon(instance.id),
      api.readInstanceAvatar(instance.id),
    ])
      .then(([props, iconDataUri, avatarDataUri]) => {
        setValues({ ...DEFAULTS, ...props });
        setIcon(iconDataUri);
        setAvatar(avatarDataUri);
      })
      .catch((err) => toast.error("Failed to load server settings", { description: String(err) }))
      .finally(() => setIsLoading(false));
  }, [instance.id]);

  useEffect(() => {
    api.listAvatarPresets().then(setAvatarPresets).catch(() => {});
  }, []);

  function set(key: string, value: string) {
    setValues((prev) => ({ ...prev, [key]: value }));
  }

  async function handleSave(e: React.FormEvent) {
    e.preventDefault();
    setIsSaving(true);
    try {
      await api.writeServerProperties(instance.id, values);
      toast.success("Server settings saved");
    } catch (err) {
      toast.error("Failed to save server settings", { description: String(err) });
    } finally {
      setIsSaving(false);
    }
  }

  async function handlePickIcon() {
    const path = await openDialog({
      title: "Choose a server icon (64x64 PNG)",
      multiple: false,
      directory: false,
      filters: [{ name: "PNG image", extensions: ["png"] }],
    });
    if (typeof path !== "string") return;

    setIsIconBusy(true);
    try {
      await api.setServerIcon(instance.id, path);
      setIcon(await api.readServerIcon(instance.id));
      toast.success("Server icon updated");
    } catch (err) {
      toast.error("Failed to set server icon", { description: String(err) });
    } finally {
      setIsIconBusy(false);
    }
  }

  async function handleClearIcon() {
    setIsIconBusy(true);
    try {
      await api.clearServerIcon(instance.id);
      setIcon(null);
    } catch (err) {
      toast.error("Failed to remove server icon", { description: String(err) });
    } finally {
      setIsIconBusy(false);
    }
  }

  async function handlePickAvatar() {
    const path = await openDialog({
      title: "Choose a profile picture",
      multiple: false,
      directory: false,
      filters: [{ name: "Image", extensions: ["png", "jpg", "jpeg", "webp", "gif"] }],
    });
    if (typeof path !== "string") return;

    setIsAvatarBusy(true);
    try {
      await api.setInstanceAvatar(instance.id, path);
      setAvatar(await api.readInstanceAvatar(instance.id));
      toast.success("Profile picture updated");
    } catch (err) {
      toast.error("Failed to set profile picture", { description: String(err) });
    } finally {
      setIsAvatarBusy(false);
    }
  }

  async function handleClearAvatar() {
    setIsAvatarBusy(true);
    try {
      await api.clearInstanceAvatar(instance.id);
      setAvatar(null);
    } catch (err) {
      toast.error("Failed to remove profile picture", { description: String(err) });
    } finally {
      setIsAvatarBusy(false);
    }
  }

  async function handlePickAvatarPreset(preset: AvatarPresetInfo) {
    setIsAvatarBusy(true);
    try {
      await api.setInstanceAvatarPreset(instance.id, preset.id);
      setAvatar(await api.readInstanceAvatar(instance.id));
      toast.success(`Profile picture set to "${preset.name}"`);
    } catch (err) {
      toast.error("Failed to set profile picture", { description: String(err) });
    } finally {
      setIsAvatarBusy(false);
    }
  }

  if (isLoading) {
    return (
      <div className="rounded-xl border border-border bg-card p-4 text-sm text-muted-foreground">
        Loading…
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
        <div>
          <h2 className="text-sm font-medium">Profile Picture</h2>
          <p className="text-xs text-muted-foreground">
            Shown on this server's card and detail page inside ModpackPilot. Any image works -
            this is separate from the multiplayer server icon below.
          </p>
        </div>
        <div className="flex items-center gap-3">
          {avatar ? (
            <img
              src={avatar}
              alt=""
              className="size-16 rounded-lg border border-border object-cover"
            />
          ) : (
            <ServerAvatarPlaceholder className="size-16 rounded-lg border border-border" />
          )}
          <div className="flex gap-2">
            <Button variant="outline" size="sm" disabled={isAvatarBusy} onClick={handlePickAvatar}>
              <ImagePlus />
              Choose Image
            </Button>
            {avatar && (
              <Button variant="outline" size="sm" disabled={isAvatarBusy} onClick={handleClearAvatar}>
                <Trash2 />
                Remove
              </Button>
            )}
          </div>
        </div>
        {avatarPresets.length > 0 && (
          <div className="flex flex-col gap-1.5">
            <p className="text-xs text-muted-foreground">Or pick a built-in picture</p>
            <div className="flex flex-wrap gap-2">
              {avatarPresets.map((preset) => (
                <button
                  key={preset.id}
                  type="button"
                  title={preset.name}
                  disabled={isAvatarBusy}
                  onClick={() => handlePickAvatarPreset(preset)}
                  className="size-12 shrink-0 overflow-hidden rounded-lg border border-border transition-colors hover:border-primary disabled:opacity-50"
                >
                  <img src={preset.dataUri} alt={preset.name} className="size-full object-cover" />
                </button>
              ))}
            </div>
          </div>
        )}
      </section>

      <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
        <div>
          <h2 className="text-sm font-medium">Server Icon</h2>
          <p className="text-xs text-muted-foreground">
            Shown to players in their multiplayer server list. Must be exactly 64x64 PNG.
          </p>
        </div>
        <div className="flex items-center gap-3">
          {icon ? (
            <img src={icon} alt="" className="size-16 rounded-lg border border-border" />
          ) : (
            <div className="flex size-16 items-center justify-center rounded-lg border border-dashed border-border text-[10px] text-muted-foreground">
              None
            </div>
          )}
          <div className="flex gap-2">
            <Button variant="outline" size="sm" disabled={isIconBusy} onClick={handlePickIcon}>
              <ImagePlus />
              Choose Image
            </Button>
            {icon && (
              <Button variant="outline" size="sm" disabled={isIconBusy} onClick={handleClearIcon}>
                <Trash2 />
                Remove
              </Button>
            )}
          </div>
        </div>
      </section>

      <form
        onSubmit={handleSave}
        className="flex flex-col gap-4 rounded-xl border border-border bg-card p-4"
      >
        <div>
          <h2 className="text-sm font-medium">Server Settings</h2>
          <p className="text-xs text-muted-foreground">
            Edits server.properties directly. Most changes take effect on the next start.
          </p>
        </div>

        <div className="flex flex-col gap-1.5">
          <Label htmlFor="prop-motd">
            MOTD <span className="text-muted-foreground">(message of the day)</span>
          </Label>
          <Input
            id="prop-motd"
            value={values.motd}
            onChange={(e) => set("motd", e.target.value)}
            placeholder="A Minecraft Server"
          />
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div className="flex flex-col gap-1.5">
            <Label>Difficulty</Label>
            <Select value={values.difficulty} onValueChange={(v) => v && set("difficulty", v)}>
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {["peaceful", "easy", "normal", "hard"].map((d) => (
                  <SelectItem key={d} value={d}>
                    {d}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
          <div className="flex flex-col gap-1.5">
            <Label>Default Gamemode</Label>
            <Select value={values.gamemode} onValueChange={(v) => v && set("gamemode", v)}>
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {["survival", "creative", "adventure", "spectator"].map((g) => (
                  <SelectItem key={g} value={g}>
                    {g}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        </div>

        <div className="grid grid-cols-3 gap-3">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="prop-max-players">Max Players</Label>
            <Input
              id="prop-max-players"
              type="number"
              min={1}
              value={values["max-players"]}
              onChange={(e) => set("max-players", e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="prop-view-distance">View Distance</Label>
            <Input
              id="prop-view-distance"
              type="number"
              min={3}
              max={32}
              value={values["view-distance"]}
              onChange={(e) => set("view-distance", e.target.value)}
            />
          </div>
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="prop-spawn-protection">Spawn Protection</Label>
            <Input
              id="prop-spawn-protection"
              type="number"
              min={0}
              value={values["spawn-protection"]}
              onChange={(e) => set("spawn-protection", e.target.value)}
            />
          </div>
        </div>

        <div className="grid grid-cols-2 gap-3">
          <div className="flex items-center justify-between">
            <Label htmlFor="prop-pvp">PVP</Label>
            <Switch
              id="prop-pvp"
              checked={values.pvp === "true"}
              onCheckedChange={(v) => set("pvp", String(v))}
            />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="prop-online-mode">Online Mode</Label>
            <Switch
              id="prop-online-mode"
              checked={values["online-mode"] === "true"}
              onCheckedChange={(v) => set("online-mode", String(v))}
            />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="prop-whitelist">Whitelist Enabled</Label>
            <Switch
              id="prop-whitelist"
              checked={values["white-list"] === "true"}
              onCheckedChange={(v) => set("white-list", String(v))}
            />
          </div>
          <div className="flex items-center justify-between">
            <Label htmlFor="prop-hardcore">Hardcore</Label>
            <Switch
              id="prop-hardcore"
              checked={values.hardcore === "true"}
              onCheckedChange={(v) => set("hardcore", String(v))}
            />
          </div>
        </div>

        <Button type="submit" disabled={isSaving} className="w-fit">
          {isSaving ? "Saving…" : "Save Server Settings"}
        </Button>
      </form>
    </div>
  );
}
