/** Mirrors Rust's `mods::health` and `mods::metadata` types. */

export type ModLoaderKind = "forge" | "neoforge" | "fabric" | "quilt";

export type ModEnvironment = "both" | "client" | "server" | "unknown";

export type Severity = "critical" | "warning" | "info";

export type FindingKind =
  | "missingDependency"
  | "duplicateMod"
  | "invalidJar"
  | "loaderMismatch"
  | "minecraftVersionMismatch"
  | "clientOnlyMod"
  | "disabledMod";

/** `notChecked` is not a pass. It means the information needed simply
 * wasn't available - a Forge JAR declares no side, an instance may have no
 * recorded Minecraft version - and the UI must say so rather than showing a
 * tick. */
export type CheckStatus = "ok" | "warning" | "critical" | "notChecked";

export interface ModDependency {
  modId: string;
  required: boolean;
  versionRange: string | null;
}

export interface ModMetadata {
  fileName: string;
  displayName: string | null;
  modId: string | null;
  version: string | null;
  loader: ModLoaderKind | null;
  environment: ModEnvironment;
  dependencies: ModDependency[];
  enabled: boolean;
  sizeBytes: number;
  /** Set when the JAR could not be read or carries no mod manifest. */
  error: string | null;
}

export interface Finding {
  kind: FindingKind;
  severity: Severity;
  summary: string;
  detail: string;
  fileNames: string[];
}

export interface CheckSummary {
  label: string;
  value: string;
  status: CheckStatus;
  /** Why a `notChecked` check couldn't run. */
  note: string | null;
}

export interface ModpackHealth {
  totalMods: number;
  enabledMods: number;
  disabledMods: number;
  unreadableMods: number;
  checks: CheckSummary[];
  findings: Finding[];
  mods: ModMetadata[];
}
