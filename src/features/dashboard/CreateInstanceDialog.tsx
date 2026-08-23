import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Plus } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useInstances } from "@/hooks/useInstances";
import { SERVER_LOADERS, type ServerLoader } from "@/types/instance";
import { getRequiredJavaMajor } from "@/lib/javaRequirement";
import { api } from "@/lib/tauri";

const LOADER_LABELS: Record<ServerLoader, string> = {
  vanilla: "Vanilla",
  forge: "Forge",
  neoforge: "NeoForge",
  fabric: "Fabric",
  quilt: "Quilt",
  unknown: "Unknown",
};

const initialState = {
  name: "",
  minecraftVersion: "",
  loader: "vanilla" as ServerLoader,
  minRamMb: "2048",
  maxRamMb: "4096",
};

export function CreateInstanceDialog() {
  const { createInstance } = useInstances();
  const [open, setOpen] = useState(false);
  const [form, setForm] = useState(initialState);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const requiredJava = getRequiredJavaMajor(form.minecraftVersion);

  // Pre-fill from Settings > New Instance Defaults, but only while the form
  // still has its untouched placeholder values - don't clobber something
  // the user already typed if this fetch resolves late.
  useEffect(() => {
    Promise.all([
      api.getAppSetting("default_min_ram_mb"),
      api.getAppSetting("default_max_ram_mb"),
    ]).then(([min, max]) => {
      setForm((prev) => ({
        ...prev,
        minRamMb: prev.minRamMb === initialState.minRamMb ? (min ?? prev.minRamMb) : prev.minRamMb,
        maxRamMb: prev.maxRamMb === initialState.maxRamMb ? (max ?? prev.maxRamMb) : prev.maxRamMb,
      }));
    });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    if (!form.name.trim()) {
      toast.error("Instance name is required");
      return;
    }

    setIsSubmitting(true);
    try {
      const instance = await createInstance({
        name: form.name.trim(),
        minecraftVersion: form.minecraftVersion.trim() || null,
        loader: form.loader,
        minRamMb: Number(form.minRamMb) || undefined,
        maxRamMb: Number(form.maxRamMb) || undefined,
      });
      toast.success(`Created "${instance.name}"`);
      setForm(initialState);
      setOpen(false);
    } catch (err) {
      toast.error("Failed to create instance", { description: String(err) });
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger render={<Button variant="outline" />}>
        <Plus />
        Create Server
      </DialogTrigger>
      <DialogContent>
        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>Create Server</DialogTitle>
            <DialogDescription>
              Sets up an empty instance directory. You'll add a server JAR and
              configure it afterward.
            </DialogDescription>
          </DialogHeader>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="instance-name">Name</Label>
            <Input
              id="instance-name"
              autoFocus
              value={form.name}
              onChange={(e) => setForm({ ...form, name: e.target.value })}
              placeholder="All The Mods 10"
            />
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="instance-mc-version">Minecraft version</Label>
            <Input
              id="instance-mc-version"
              value={form.minecraftVersion}
              onChange={(e) =>
                setForm({ ...form, minecraftVersion: e.target.value })
              }
              placeholder="1.21.1 (optional)"
            />
            {requiredJava !== null && (
              <p className="text-xs text-muted-foreground">
                Requires Java {requiredJava}. You can assign it after creating the instance.
              </p>
            )}
          </div>

          <div className="flex flex-col gap-1.5">
            <Label>Loader</Label>
            <Select
              value={form.loader}
              onValueChange={(value) =>
                setForm({ ...form, loader: value as ServerLoader })
              }
            >
              <SelectTrigger className="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {SERVER_LOADERS.map((loader) => (
                  <SelectItem key={loader} value={loader}>
                    {LOADER_LABELS[loader]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="instance-min-ram">Min RAM (MB)</Label>
              <Input
                id="instance-min-ram"
                type="number"
                min={512}
                step={512}
                value={form.minRamMb}
                onChange={(e) =>
                  setForm({ ...form, minRamMb: e.target.value })
                }
              />
            </div>
            <div className="flex flex-col gap-1.5">
              <Label htmlFor="instance-max-ram">Max RAM (MB)</Label>
              <Input
                id="instance-max-ram"
                type="number"
                min={512}
                step={512}
                value={form.maxRamMb}
                onChange={(e) =>
                  setForm({ ...form, maxRamMb: e.target.value })
                }
              />
            </div>
          </div>

          <DialogFooter>
            <Button type="submit" disabled={isSubmitting}>
              {isSubmitting ? "Creating…" : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
