import { useState } from "react";
import { toast } from "sonner";
import {
  AlertTriangle,
  CheckCircle2,
  ChevronDown,
  CircleSlash,
  Info,
  Stethoscope,
  XCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import type { Instance } from "@/types/instance";
import type {
  CheckStatus,
  Finding,
  ModpackHealth,
  Severity,
} from "@/types/modpackHealth";

/** Tinted classes per status, following the same idiom as
 * `STATUS_BADGE_CLASS` for server status. `notChecked` is deliberately the
 * muted treatment rather than a green tick - it is an absence of
 * information, not a pass. */
const CHECK_STYLE: Record<CheckStatus, { icon: typeof CheckCircle2; className: string }> = {
  ok: { icon: CheckCircle2, className: "text-primary" },
  warning: { icon: AlertTriangle, className: "text-amber-600 dark:text-amber-400" },
  critical: { icon: XCircle, className: "text-destructive" },
  notChecked: { icon: CircleSlash, className: "text-muted-foreground" },
};

const SEVERITY_STYLE: Record<Severity, { icon: typeof CheckCircle2; className: string; label: string }> = {
  critical: { icon: XCircle, className: "text-destructive", label: "Critical" },
  warning: {
    icon: AlertTriangle,
    className: "text-amber-600 dark:text-amber-400",
    label: "Warning",
  },
  info: { icon: Info, className: "text-muted-foreground", label: "Info" },
};

function FindingRow({ finding }: { finding: Finding }) {
  const [expanded, setExpanded] = useState(false);
  const { icon: Icon, className, label } = SEVERITY_STYLE[finding.severity];

  return (
    <li className="rounded-lg border border-border">
      <button
        type="button"
        className="flex w-full items-start gap-2.5 p-3 text-left"
        onClick={() => setExpanded((v) => !v)}
        aria-expanded={expanded}
      >
        <Icon className={cn("mt-0.5 size-4 shrink-0", className)} aria-label={label} />
        <span className="flex-1 text-sm">{finding.summary}</span>
        <ChevronDown
          className={cn(
            "mt-0.5 size-4 shrink-0 text-muted-foreground transition-transform",
            expanded && "rotate-180",
          )}
        />
      </button>
      {expanded && (
        <div className="flex flex-col gap-2 border-t border-border px-3 py-2.5">
          <p className="text-xs text-muted-foreground">{finding.detail}</p>
          {finding.fileNames.length > 0 && (
            <ul className="flex flex-col gap-0.5">
              {finding.fileNames.map((name) => (
                <li key={name} className="font-mono text-xs break-all">
                  {name}
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </li>
  );
}

/**
 * On-demand modpack health report.
 *
 * Deliberately not run automatically on mount: the scan opens every JAR in
 * the mods folder, which on a 300-mod pack is real work, and silently doing
 * it every time someone clicks the Mods tab would make the tab feel broken.
 */
export function ModpackHealthCard({
  instance,
  onResult,
}: {
  instance: Instance;
  /** Lets the parent annotate its mod rows with the findings. */
  onResult?: (health: ModpackHealth) => void;
}) {
  const [health, setHealth] = useState<ModpackHealth | null>(null);
  const [isScanning, setIsScanning] = useState(false);

  async function handleScan() {
    setIsScanning(true);
    try {
      const result = await api.analyzeModpackHealth(instance.id);
      setHealth(result);
      onResult?.(result);
    } catch (err) {
      toast.error("Modpack check failed", { description: String(err) });
    } finally {
      setIsScanning(false);
    }
  }

  const criticalCount = health?.findings.filter((f) => f.severity === "critical").length ?? 0;
  const warningCount = health?.findings.filter((f) => f.severity === "warning").length ?? 0;

  return (
    <section className="flex flex-col gap-3 rounded-xl border border-border bg-card p-4">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h2 className="text-sm font-medium">Modpack Health</h2>
          <p className="text-xs text-muted-foreground">
            Reads what each JAR declares about itself. Nothing is changed —
            every fix stays your decision.
          </p>
        </div>
        <Button size="sm" variant="outline" disabled={isScanning} onClick={handleScan}>
          <Stethoscope />
          {isScanning ? "Checking…" : health ? "Re-check" : "Check Modpack"}
        </Button>
      </div>

      {health && (
        <>
          <div className="flex flex-wrap items-center gap-1.5">
            {criticalCount > 0 && (
              <Badge className="border-transparent bg-destructive/15 text-destructive">
                {criticalCount} critical
              </Badge>
            )}
            {warningCount > 0 && (
              <Badge className="border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-400">
                {warningCount} warning{warningCount === 1 ? "" : "s"}
              </Badge>
            )}
            {criticalCount === 0 && warningCount === 0 && (
              <Badge className="border-transparent bg-primary/15 text-primary">
                No problems found
              </Badge>
            )}
            <Badge variant="secondary">
              {health.enabledMods} enabled
              {health.disabledMods > 0 ? ` · ${health.disabledMods} disabled` : ""}
            </Badge>
          </div>

          <dl className="grid gap-x-6 gap-y-2 sm:grid-cols-2">
            {health.checks.map((check) => {
              const { icon: Icon, className } = CHECK_STYLE[check.status];
              return (
                <div key={check.label} className="flex items-start gap-2 text-sm">
                  <Icon className={cn("mt-0.5 size-4 shrink-0", className)} />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-baseline justify-between gap-2">
                      <dt className="text-muted-foreground">{check.label}</dt>
                      <dd className="truncate font-medium">{check.value}</dd>
                    </div>
                    {check.note && (
                      <p className="mt-0.5 text-xs text-muted-foreground">{check.note}</p>
                    )}
                  </div>
                </div>
              );
            })}
          </dl>

          {health.findings.length > 0 && (
            <ul className="flex flex-col gap-1.5">
              {health.findings.map((finding, i) => (
                <FindingRow key={`${finding.kind}-${i}`} finding={finding} />
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}
