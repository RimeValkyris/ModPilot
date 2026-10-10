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
import { useInstancesStore } from "@/stores/instancesStore";
import {
  SERVER_LOADERS,
  isBuildToolsLoader,
  isDirectDownloadLoader,
  type ServerLoader,
} from "@/types/instance";
import { getRequiredJavaMajor } from "@/lib/javaRequirement";
import { api } from "@/lib/tauri";
import { JavaRequirementNotice } from "@/features/dashboard/JavaRequirementNotice";

const LOADER_LABELS: Record<ServerLoader, string> = {
  vanilla: "Vanilla",
  forge: "Forge",
  neoforge: "NeoForge",
  fabric: "Fabric",
  quilt: "Quilt",
  paper: "Paper",
  purpur: "Purpur",
  spigot: "Spigot",
  bukkit: "Bukkit",
  unknown: "Unknown",
};

/** One line under the picker so the choice isn't a guess. */
const LOADER_HINTS: Partial<Record<ServerLoader, string>> = {
  vanilla: "Official Mojang server. No plugins or mods.",
  paper: "Fast, plugin-compatible (Spigot/Bukkit plugins work).",
  purpur: "Paper fork with extra gameplay options. Spigot/Bukkit plugins work.",
  spigot: "Original plugin server. Built on your PC (5-15 min) after creating.",
  bukkit: "CraftBukkit, plugin API only. Built on your PC (5-15 min) after creating.",
  forge: "Mods. Install the server after creating, or import a pack.",
  neoforge: "Mods. Install the server after creating, or import a pack.",
  fabric: "Mods. You'll add the server jar after creating.",
  quilt: "Mods. You'll add the server jar after creating.",
};

const initialState = {
  name: "",
  minecraftVersion: "",
  loader: "vanilla" as ServerLoader,
  minRamMb: "2048",
  maxRamMb: "4096",
};

export function CreateInstanceDialog() {
  const { createInstance, installServerJar } = useInstancesStore();
  const [open, setOpen] = useState(false);
  const [form, setForm] = useState(initialState);
  const [isSubmitting, setIsSubmitting] = useState(false);
  const [acceptedEula, setAcceptedEula] = useState(false);
  const [submitStep, setSubmitStep] = useState<"creating" | "downloading">("creating");
  const requiredJava = getRequiredJavaMajor(form.minecraftVersion);

  const downloadable = isDirectDownloadLoader(form.loader);
  const [versions, setVersions] = useState<string[]>([]);
  const [versionsState, setVersionsState] = useState<"idle" | "loading" | "error">("idle");

  // Downloadable server types get a dropdown of the versions the vendor
  // really publishes. A failed fetch (offline) falls back to the free-text
  // field below, so creating a server never depends on these APIs.
  useEffect(() => {
    if (!open || !downloadable) return;
    let cancelled = false;
    setVersionsState("loading");
    setVersions([]);
    api
      .listServerVersions(form.loader)
      .then((list) => {
        if (cancelled) return;
        setVersions(list);
        setVersionsState("idle");
        // Default to the newest, but never replace something already chosen
        // that this server type also supports.
        setForm((prev) => ({
          ...prev,
          minecraftVersion: list.includes(prev.minecraftVersion)
            ? prev.minecraftVersion
            : (list[0] ?? ""),
        }));
      })
      .catch(() => {
        if (!cancelled) setVersionsState("error");
      });
    return () => {
      cancelled = true;
    };
  }, [open, downloadable, form.loader]);

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

    const version = form.minecraftVersion.trim();
    if (downloadable && !version) {
      toast.error("Choose a Minecraft version");
      return;
    }
    if (downloadable && !acceptedEula) {
      toast.error("Accept the Minecraft EULA to create this server");
      return;
    }

    setIsSubmitting(true);
    setSubmitStep("creating");
    try {
      const instance = await createInstance({
        name: form.name.trim(),
        minecraftVersion: version || null,
        loader: form.loader,
        minRamMb: Number(form.minRamMb) || undefined,
        maxRamMb: Number(form.maxRamMb) || undefined,
      });
      if (isBuildToolsLoader(form.loader)) {
        // A BuildTools run takes minutes, so it isn't done inside this
        // dialog: the server's page shows the build with live progress.
        try {
          await api.acceptEula(instance.id);
        } catch (err) {
          toast.warning("Couldn't record the EULA acceptance", { description: String(err) });
        }
        toast.success(`Created "${instance.name}"`, {
          description: `Open it and press "Build Server" to compile ${LOADER_LABELS[form.loader]} ${version}.`,
        });
      } else if (downloadable) {
        setSubmitStep("downloading");
        try {
          await api.acceptEula(instance.id);
          await installServerJar(instance.id);
          toast.success(`Created "${instance.name}"`, {
            description: `${LOADER_LABELS[form.loader]} ${version} is downloaded and ready to start.`,
          });
        } catch (err) {
          // The instance exists; the console page offers a retry.
          toast.warning(`Created "${instance.name}", but the download failed`, {
            description: `${String(err)} Open the server to retry.`,
          });
        }
      } else {
        toast.success(`Created "${instance.name}"`);
      }
      setForm(initialState);
      setAcceptedEula(false);
      setOpen(false);
    } catch (err) {
      toast.error("Failed to create instance", { description: String(err) });
    } finally {
      setIsSubmitting(false);
    }
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next && isSubmitting) return;
        setOpen(next);
      }}
    >
      <DialogTrigger render={<Button variant="outline" />}>
        <Plus />
        Create Server
      </DialogTrigger>
      <DialogContent showCloseButton={!isSubmitting}>
        <form onSubmit={handleSubmit} className="flex flex-col gap-4">
          <DialogHeader>
            <DialogTitle>Create Server</DialogTitle>
            <DialogDescription>
              Pick a server type and Minecraft version. Vanilla, Paper and
              Purpur are downloaded for you; Spigot and Bukkit are built.
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
            <Label>Server type</Label>
            <Select
              value={form.loader}
              onValueChange={(value) =>
                setForm({ ...form, loader: value as ServerLoader })
              }
            >
              <SelectTrigger className="w-full">
                <SelectValue>{(value: ServerLoader) => LOADER_LABELS[value]}</SelectValue>
              </SelectTrigger>
              <SelectContent>
                {SERVER_LOADERS.map((loader) => (
                  <SelectItem key={loader} value={loader}>
                    {LOADER_LABELS[loader]}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
            {LOADER_HINTS[form.loader] && (
              <p className="text-xs text-muted-foreground">{LOADER_HINTS[form.loader]}</p>
            )}
          </div>

          <div className="flex flex-col gap-1.5">
            <Label htmlFor="instance-mc-version">Minecraft version</Label>
            {downloadable && versionsState !== "error" ? (
              <Select
                value={form.minecraftVersion}
                onValueChange={(value) =>
                  setForm({ ...form, minecraftVersion: value as string })
                }
                disabled={versionsState === "loading"}
              >
                <SelectTrigger id="instance-mc-version" className="w-full">
                  <SelectValue
                    placeholder={versionsState === "loading" ? "Loading versions…" : "Select a version"}
                  />
                </SelectTrigger>
                <SelectContent>
                  {versions.map((version) => (
                    <SelectItem key={version} value={version}>
                      {version}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            ) : (
              <>
                <Input
                  id="instance-mc-version"
                  value={form.minecraftVersion}
                  onChange={(e) =>
                    setForm({ ...form, minecraftVersion: e.target.value })
                  }
                  placeholder={downloadable ? "1.21.1" : "1.21.1 (optional)"}
                />
                {versionsState === "error" && (
                  <p className="text-xs text-muted-foreground">
                    Couldn't load the version list (offline?). Type a version instead.
                  </p>
                )}
              </>
            )}
            {requiredJava !== null && /^\d+(\.\d+)+$/.test(form.minecraftVersion.trim()) && (
              <JavaRequirementNotice minecraftVersion={form.minecraftVersion.trim()} />
            )}
          </div>

          {downloadable && (
            <label className="flex items-start gap-2 text-xs text-muted-foreground">
              <input
                type="checkbox"
                className="mt-0.5 size-3.5 shrink-0 accent-primary"
                checked={acceptedEula}
                onChange={(e) => setAcceptedEula(e.target.checked)}
              />
              <span>
                I accept the{" "}
                <a
                  href="https://aka.ms/MinecraftEULA"
                  target="_blank"
                  rel="noopener noreferrer"
                  className="underline"
                >
                  Minecraft EULA
                </a>
                . Servers won't start until it's accepted.
              </span>
            </label>
          )}

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
            <Button
              type="submit"
              disabled={isSubmitting || (downloadable && versionsState === "loading")}
            >
              {isSubmitting
                ? submitStep === "downloading"
                  ? "Downloading server…"
                  : "Creating…"
                : "Create"}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  );
}
