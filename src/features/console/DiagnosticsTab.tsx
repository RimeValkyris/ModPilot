import { useState } from "react";
import { toast } from "sonner";
import {
  AlertTriangle,
  CheckCircle2,
  CircleSlash,
  Stethoscope,
  XCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { api } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { LaunchHistoryCard } from "@/features/console/LaunchHistoryCard";
import type { Instance } from "@/types/instance";
import type {
  DiagnosticReport,
  DiagnosticStatus,
  LogLevel,
} from "@/types/diagnostics";

const STATUS_STYLE: Record<
  DiagnosticStatus,
  { icon: typeof CheckCircle2; className: string; label: string }
> = {
  ok: { icon: CheckCircle2, className: "text-primary", label: "OK" },
  warning: {
    icon: AlertTriangle,
    className: "text-amber-600 dark:text-amber-400",
    label: "Warning",
  },
  critical: { icon: XCircle, className: "text-destructive", label: "Critical" },
  // Muted, never a tick: this check did not run.
  notApplicable: {
    icon: CircleSlash,
    className: "text-muted-foreground",
    label: "Not checked",
  },
};

const OVERALL_BADGE: Record<DiagnosticStatus, { className: string; label: string }> = {
  ok: { className: "border-transparent bg-primary/15 text-primary", label: "HEALTHY" },
  warning: {
    className: "border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-400",
    label: "WARNING",
  },
  critical: {
    className: "border-transparent bg-destructive/15 text-destructive",
    label: "CRITICAL",
  },
  notApplicable: {
    className: "border-transparent bg-muted text-muted-foreground",
    label: "NOT CHECKED",
  },
};

const LOG_LEVEL_STYLE: Record<LogLevel, string> = {
  fatal: "border-transparent bg-destructive/15 text-destructive",
  error: "border-transparent bg-destructive/15 text-destructive",
  warn: "border-transparent bg-amber-500/15 text-amber-600 dark:text-amber-400",
};

/**
 * Runs every diagnostic check against an instance and explains the results.
 *
 * On demand rather than on mount: the scan reads every mod JAR and the tail
 * of the server log, which is real work and shouldn't happen just because
 * someone clicked through a tab.
 */
export function DiagnosticsTab({ instance }: { instance: Instance }) {
  const [report, setReport] = useState<DiagnosticReport | null>(null);
  const [isRunning, setIsRunning] = useState(false);

  async function handleRun() {
    setIsRunning(true);
    try {
      setReport(await api.runDiagnostics(instance.id));
    } catch (err) {
      toast.error("Diagnostics failed", { description: String(err) });
    } finally {
      setIsRunning(false);
    }
  }

  return (
    <div className="flex flex-col gap-4">
      <section className="flex items-start justify-between gap-3 rounded-xl border border-border bg-card p-4">
        <div>
          <h2 className="text-sm font-medium">Diagnostics</h2>
          <p className="text-xs text-muted-foreground">
            Checks Java, memory, disk, mods, recent log errors and crash
            history. Read-only — running this changes nothing.
          </p>
        </div>
        <Button size="sm" disabled={isRunning} onClick={handleRun}>
          <Stethoscope />
          {isRunning ? "Running…" : report ? "Run Again" : "Run Diagnostics"}
        </Button>
      </section>

      <LaunchHistoryCard instance={instance} />

      {report && (
        <>
          <section className="flex flex-wrap items-center gap-2 rounded-xl border border-border bg-card p-4">
            <Badge className={OVERALL_BADGE[report.overall].className}>
              {OVERALL_BADGE[report.overall].label}
            </Badge>
            <span className="text-xs text-muted-foreground">
              Checked {new Date(report.generatedAt).toLocaleString()}
            </span>
          </section>

          <ul className="flex flex-col gap-2">
            {report.diagnostics.map((diagnostic) => {
              const { icon: Icon, className, label } = STATUS_STYLE[diagnostic.status];
              return (
                <li
                  key={diagnostic.id}
                  className="flex items-start gap-2.5 rounded-lg border border-border bg-card p-3"
                >
                  <Icon
                    className={cn("mt-0.5 size-4 shrink-0", className)}
                    aria-label={label}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-baseline gap-x-2">
                      <span className="text-sm font-medium">{diagnostic.label}</span>
                      <span className="text-sm text-muted-foreground">
                        {diagnostic.summary}
                      </span>
                    </div>
                    {diagnostic.detail && (
                      <p className="mt-1 text-xs text-muted-foreground">
                        {diagnostic.detail}
                      </p>
                    )}
                  </div>
                </li>
              );
            })}
          </ul>

          {report.logIssues.length > 0 && (
            <section className="flex flex-col gap-2 rounded-xl border border-border bg-card p-4">
              <div>
                <h2 className="text-sm font-medium">Errors in the recent log</h2>
                <p className="text-xs text-muted-foreground">
                  Grouped by shape, so one fault repeating with different
                  coordinates counts once. Highest count first — that's usually
                  the real problem.
                </p>
              </div>
              <ul className="flex flex-col gap-2">
                {report.logIssues.map((issue, i) => (
                  <li key={`${issue.example}-${i}`} className="rounded-lg border border-border p-2.5">
                    <div className="mb-1 flex flex-wrap items-center gap-1.5">
                      <Badge className={LOG_LEVEL_STYLE[issue.level]}>
                        {issue.level.toUpperCase()}
                      </Badge>
                      <Badge variant="secondary">
                        ×{issue.count}
                      </Badge>
                      {issue.modId && <Badge variant="outline">{issue.modId}</Badge>}
                    </div>
                    <p className="font-mono text-xs break-all text-muted-foreground">
                      {issue.example}
                    </p>
                  </li>
                ))}
              </ul>
            </section>
          )}
        </>
      )}
    </div>
  );
}
