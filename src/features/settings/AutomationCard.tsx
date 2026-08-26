import { useState } from "react";
import { toast } from "sonner";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useInstancesStore } from "@/stores/instancesStore";
import type { Instance } from "@/types/instance";

const OFF = "off";
const EVERY = "every";
const DAILY = "daily";

type Mode = typeof OFF | typeof EVERY | typeof DAILY;

/** Splits the stored `"every:6"` / `"daily:04:00"` form into UI state. */
function parseSchedule(raw: string | null): { mode: Mode; hours: string; time: string } {
  if (!raw) return { mode: OFF, hours: "6", time: "04:00" };
  const [kind, ...rest] = raw.split(":");
  const value = rest.join(":");
  if (kind === EVERY) return { mode: EVERY, hours: value || "6", time: "04:00" };
  if (kind === DAILY) return { mode: DAILY, hours: "6", time: value || "04:00" };
  return { mode: OFF, hours: "6", time: "04:00" };
}

function buildSchedule(mode: Mode, hours: string, time: string): string | null {
  if (mode === EVERY) {
    const n = Number(hours);
    return Number.isFinite(n) && n > 0 ? `every:${Math.floor(n)}` : null;
  }
  if (mode === DAILY) return `daily:${time}`;
  return null;
}

function ScheduleFields({
  idPrefix,
  mode,
  hours,
  time,
  onChange,
}: {
  idPrefix: string;
  mode: Mode;
  hours: string;
  time: string;
  onChange: (next: { mode: Mode; hours: string; time: string }) => void;
}) {
  return (
    <div className="flex flex-wrap items-end gap-2">
      <div className="flex flex-col gap-1.5">
        <Select value={mode} onValueChange={(v) => v && onChange({ mode: v as Mode, hours, time })}>
          <SelectTrigger className="w-40" size="sm">
            <SelectValue />
          </SelectTrigger>
          <SelectContent>
            <SelectItem value={OFF} label="Off">
              Off
            </SelectItem>
            <SelectItem value={EVERY} label="Every N hours">
              Every N hours
            </SelectItem>
            <SelectItem value={DAILY} label="Daily at…">
              Daily at…
            </SelectItem>
          </SelectContent>
        </Select>
      </div>

      {mode === EVERY && (
        <div className="flex flex-col gap-1.5">
          <Label htmlFor={`${idPrefix}-hours`} className="text-xs">
            Hours
          </Label>
          <Input
            id={`${idPrefix}-hours`}
            type="number"
            min={1}
            step={1}
            className="h-8 w-24"
            value={hours}
            onChange={(e) => onChange({ mode, hours: e.target.value, time })}
          />
        </div>
      )}

      {mode === DAILY && (
        <div className="flex flex-col gap-1.5">
          <Label htmlFor={`${idPrefix}-time`} className="text-xs">
            Time (local)
          </Label>
          <Input
            id={`${idPrefix}-time`}
            type="time"
            className="h-8 w-32"
            value={time}
            onChange={(e) => onChange({ mode, hours, time: e.target.value })}
          />
        </div>
      )}
    </div>
  );
}

export function AutomationCard({ instance }: { instance: Instance }) {
  const { setInstanceSchedules } = useInstancesStore();
  const [restart, setRestart] = useState(() => parseSchedule(instance.restartSchedule));
  const [backup, setBackup] = useState(() => parseSchedule(instance.backupSchedule));
  const [keepLast, setKeepLast] = useState(String(instance.backupKeepLast));
  const [isSaving, setIsSaving] = useState(false);

  async function handleSave() {
    setIsSaving(true);
    try {
      await setInstanceSchedules(
        instance.id,
        buildSchedule(restart.mode, restart.hours, restart.time),
        buildSchedule(backup.mode, backup.hours, backup.time),
        Math.max(0, Math.floor(Number(keepLast) || 0)),
      );
      toast.success("Automation saved");
    } catch (err) {
      toast.error("Failed to save automation", { description: String(err) });
    } finally {
      setIsSaving(false);
    }
  }

  return (
    <section className="flex flex-col gap-4 rounded-xl border border-border bg-card p-4">
      <div>
        <h2 className="text-sm font-medium">Automation</h2>
        <p className="text-xs text-muted-foreground">
          Scheduled actions only run while this server is actually running - they never start a
          stopped server on their own.
        </p>
      </div>

      <div className="flex flex-col gap-1.5">
        <Label className="text-xs font-medium">Automatic restart</Label>
        <ScheduleFields idPrefix="restart" {...restart} onChange={setRestart} />
        <p className="text-xs text-muted-foreground">
          Long-running servers drift into memory pressure and TPS loss; a periodic restart keeps
          them healthy.
        </p>
      </div>

      <div className="flex flex-col gap-1.5 border-t border-border pt-4">
        <Label className="text-xs font-medium">Automatic world backup</Label>
        <ScheduleFields idPrefix="backup" {...backup} onChange={setBackup} />
        <div className="mt-1 flex items-end gap-2">
          <div className="flex flex-col gap-1.5">
            <Label htmlFor="backup-keep" className="text-xs">
              Keep last
            </Label>
            <Input
              id="backup-keep"
              type="number"
              min={0}
              step={1}
              className="h-8 w-24"
              value={keepLast}
              onChange={(e) => setKeepLast(e.target.value)}
            />
          </div>
          <p className="pb-1.5 text-xs text-muted-foreground">
            0 keeps every backup. Rotation applies to scheduled backups only - a backup you
            created by hand is a deliberate checkpoint and is never auto-deleted.
          </p>
        </div>
      </div>

      <Button size="sm" className="w-fit" disabled={isSaving} onClick={handleSave}>
        {isSaving ? "Saving…" : "Save Automation"}
      </Button>
    </section>
  );
}
