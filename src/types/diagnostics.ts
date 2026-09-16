/** Mirrors Rust's `diagnostics::report` and `diagnostics::logs` types. */

export type LogLevel = "fatal" | "error" | "warn";

/** `notApplicable` is not a pass - the check could not run because the
 * information it needs wasn't there. The UI must show that distinctly. */
export type DiagnosticStatus = "critical" | "warning" | "ok" | "notApplicable";

export interface LogIssue {
  level: LogLevel;
  /** The real matching line, not the normalized key it was grouped by. */
  example: string;
  count: number;
  /** Set only when the message names a mod that is actually installed. */
  modId: string | null;
}

export interface Diagnostic {
  id: string;
  label: string;
  status: DiagnosticStatus;
  summary: string;
  /** Empty when the summary already says everything. */
  detail: string;
}

export interface DiagnosticReport {
  /** The worst status among the checks that actually ran. */
  overall: DiagnosticStatus;
  generatedAt: string;
  diagnostics: Diagnostic[];
  logIssues: LogIssue[];
}
