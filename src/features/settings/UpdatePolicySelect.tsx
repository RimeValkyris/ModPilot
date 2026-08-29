import { AlertTriangle } from "lucide-react";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";

/** The "when a new version is published" control.
 *
 * Shared by the Modrinth and FTB sections: the policy is stored once per
 * instance and means exactly the same thing whichever service the update
 * came from, so it would be wrong for the two to drift apart. */
export function UpdatePolicySelect({
  value,
  onChange,
}: {
  value: string;
  onChange: (policy: string) => void;
}) {
  return (
    <div className="flex flex-col gap-1.5 border-t border-border pt-3">
      <Label className="text-xs font-medium">When a new version is published</Label>
      <Select value={value} onValueChange={(v) => v && onChange(v)}>
        <SelectTrigger className="w-64" size="sm">
          <SelectValue />
        </SelectTrigger>
        <SelectContent>
          <SelectItem value="off" label="Do nothing">
            Do nothing
          </SelectItem>
          <SelectItem value="notify" label="Notify me">
            Notify me
          </SelectItem>
          <SelectItem value="auto" label="Install automatically">
            Install automatically
          </SelectItem>
        </SelectContent>
      </Select>
      {value === "auto" && (
        <p className="flex items-start gap-1.5 text-xs text-amber-600 dark:text-amber-400">
          <AlertTriangle className="mt-0.5 size-3.5 shrink-0" />
          Before installing, the server is stopped and the world is backed up. Updates are
          skipped while anyone is online, and that safety backup is never auto-deleted by
          retention. A pack update can still break an existing world - keep an eye on it.
        </p>
      )}
    </div>
  );
}
