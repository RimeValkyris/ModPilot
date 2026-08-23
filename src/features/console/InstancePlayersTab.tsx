import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Plus, Trash2 } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { api } from "@/lib/tauri";

type PlayerEntry = Record<string, unknown> & { name?: string; uuid?: string };

interface ListConfig {
  file: string;
  title: string;
  description: string;
  extraField?: { key: string; label: string; type: "level" | "text" };
}

const LISTS: ListConfig[] = [
  {
    file: "whitelist.json",
    title: "Whitelist",
    description: "Only these players can join when white-listing is enabled.",
  },
  {
    file: "ops.json",
    title: "Operators",
    description: "Players with operator (admin) permissions.",
    extraField: { key: "level", label: "Level", type: "level" },
  },
  {
    file: "banned-players.json",
    title: "Banned Players",
    description: "Players blocked from joining.",
    extraField: { key: "reason", label: "Reason", type: "text" },
  },
];

function PlayerListEditor({ instanceId, config }: { instanceId: string; config: ListConfig }) {
  const [entries, setEntries] = useState<PlayerEntry[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [isSaving, setIsSaving] = useState(false);
  const [newName, setNewName] = useState("");
  const [newUuid, setNewUuid] = useState("");
  const [newExtra, setNewExtra] = useState(config.extraField?.type === "level" ? "4" : "");

  useEffect(() => {
    setIsLoading(true);
    api
      .readPlayerList(instanceId, config.file)
      .then((data) => setEntries(Array.isArray(data) ? (data as PlayerEntry[]) : []))
      .catch((err) => toast.error(`Failed to load ${config.title}`, { description: String(err) }))
      .finally(() => setIsLoading(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [instanceId, config.file]);

  async function save(next: PlayerEntry[]) {
    setIsSaving(true);
    try {
      await api.writePlayerList(instanceId, config.file, next);
      setEntries(next);
    } catch (err) {
      toast.error(`Failed to save ${config.title}`, { description: String(err) });
    } finally {
      setIsSaving(false);
    }
  }

  function handleAdd() {
    if (!newName.trim()) {
      toast.error("Name is required");
      return;
    }
    const entry: PlayerEntry = { uuid: newUuid.trim(), name: newName.trim() };
    if (config.extraField) {
      entry[config.extraField.key] =
        config.extraField.type === "level" ? Number(newExtra) || 0 : newExtra;
    }
    if (config.file === "banned-players.json") {
      entry.created ??= new Date().toISOString();
      entry.source ??= "ModForge";
      entry.expires ??= "forever";
    }
    save([...entries, entry]);
    setNewName("");
    setNewUuid("");
  }

  function handleRemove(index: number) {
    save(entries.filter((_, i) => i !== index));
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-xs text-muted-foreground">{config.description}</p>

      {isLoading ? (
        <p className="text-sm text-muted-foreground">Loading…</p>
      ) : entries.length === 0 ? (
        <p className="rounded-lg border border-dashed border-border p-3 text-sm text-muted-foreground">
          No entries.
        </p>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {entries.map((entry, index) => (
            <li
              key={index}
              className="flex items-center justify-between gap-2 rounded-lg border border-border p-2 text-sm"
            >
              <div className="overflow-hidden">
                <p className="truncate font-medium">{String(entry.name ?? "Unknown")}</p>
                <p className="truncate text-xs text-muted-foreground">
                  {String(entry.uuid ?? "no UUID")}
                  {config.extraField && entry[config.extraField.key] !== undefined
                    ? ` · ${config.extraField.label}: ${String(entry[config.extraField.key])}`
                    : ""}
                </p>
              </div>
              <Button variant="ghost" size="icon-sm" onClick={() => handleRemove(index)}>
                <Trash2 />
              </Button>
            </li>
          ))}
        </ul>
      )}

      <div className="flex flex-wrap items-end gap-2 border-t border-border pt-3">
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">Name</label>
          <Input
            className="w-36"
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            placeholder="PlayerName"
          />
        </div>
        <div className="flex flex-col gap-1">
          <label className="text-xs text-muted-foreground">UUID (optional)</label>
          <Input
            className="w-56"
            value={newUuid}
            onChange={(e) => setNewUuid(e.target.value)}
            placeholder="Leave blank if unknown"
          />
        </div>
        {config.extraField?.type === "level" && (
          <div className="flex flex-col gap-1">
            <label className="text-xs text-muted-foreground">Level</label>
            <Select value={newExtra} onValueChange={(v) => v && setNewExtra(v)}>
              <SelectTrigger className="w-20">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                {["1", "2", "3", "4"].map((lvl) => (
                  <SelectItem key={lvl} value={lvl}>
                    {lvl}
                  </SelectItem>
                ))}
              </SelectContent>
            </Select>
          </div>
        )}
        {config.extraField?.type === "text" && (
          <div className="flex flex-col gap-1">
            <label className="text-xs text-muted-foreground">{config.extraField.label}</label>
            <Input className="w-40" value={newExtra} onChange={(e) => setNewExtra(e.target.value)} />
          </div>
        )}
        <Button size="sm" disabled={isSaving} onClick={handleAdd}>
          <Plus />
          Add
        </Button>
      </div>
    </div>
  );
}

export function InstancePlayersTab({ instanceId }: { instanceId: string }) {
  return (
    <Tabs defaultValue={LISTS[0].file}>
      <TabsList>
        {LISTS.map((list) => (
          <TabsTrigger key={list.file} value={list.file}>
            {list.title}
          </TabsTrigger>
        ))}
      </TabsList>
      {LISTS.map((list) => (
        <TabsContent key={list.file} value={list.file}>
          <div className="rounded-xl border border-border bg-card p-4">
            <PlayerListEditor instanceId={instanceId} config={list} />
          </div>
        </TabsContent>
      ))}
    </Tabs>
  );
}
