/**
 * Frontend mirror of the Rust models in `src-tauri/src/models/instance.rs`.
 * Field names are camelCase because `Instance` derives
 * `#[serde(rename_all = "camelCase")]` on the Rust side.
 */

export type ServerLoader =
  | "vanilla"
  | "forge"
  | "neoforge"
  | "fabric"
  | "quilt"
  | "unknown";

export type ServerStatus =
  | "stopped"
  | "starting"
  | "running"
  | "stopping"
  | "crashed";

export interface Instance {
  id: string;
  name: string;
  minecraftVersion: string | null;
  loader: ServerLoader;
  loaderVersion: string | null;
  javaInstallationId: string | null;
  minRamMb: number;
  maxRamMb: number;
  serverDirectory: string;
  serverJar: string | null;
  /** "jar" (default) or "argfile" - see Rust's `Instance::launch_mode`. */
  launchMode: string;
  jvmArgs: string[];
  serverArgs: string[];
  status: ServerStatus;
  autoStart: boolean;
  autoRestart: boolean;
  createdAt: string;
  lastLaunchedAt: string | null;
  modrinthProjectId: string | null;
  modrinthProjectTitle: string | null;
  modrinthVersionId: string | null;
  /** Set when this instance came from (or was linked to) an FTB modpack.
   * FTB keys packs and versions by integer ID rather than slug. */
  ftbPackId: number | null;
  ftbPackName: string | null;
  ftbVersionId: number | null;
  /** "every:6" | "daily:04:00" | null (disabled). */
  restartSchedule: string | null;
  backupSchedule: string | null;
  /** How many backups a scheduled backup keeps; 0 = keep all. */
  backupKeepLast: number;
  /** "off" | "notify" | "auto" - how new Modrinth versions are handled. */
  updatePolicy: string;
}

/** Input for `update_instance_settings` (Configuration tab). */
export interface UpdateInstanceSettingsRequest {
  serverJar: string | null;
  jvmArgs: string[];
  serverArgs: string[];
  minRamMb: number;
  maxRamMb: number;
  autoStart: boolean;
  autoRestart: boolean;
}

/** Input for `create_instance`. Mirrors Rust's `CreateInstanceRequest`. */
export interface CreateInstanceRequest {
  name: string;
  minecraftVersion?: string | null;
  loader?: ServerLoader | null;
  loaderVersion?: string | null;
  minRamMb?: number | null;
  maxRamMb?: number | null;
}

export const SERVER_LOADERS: ServerLoader[] = [
  "vanilla",
  "forge",
  "neoforge",
  "fabric",
  "quilt",
];
