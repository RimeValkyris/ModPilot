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
  | "paper"
  | "purpur"
  | "spigot"
  | "bukkit"
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
  /** "jar" (default), "argfile", "script", or "installer" (the pack shipped
   * only a Forge/NeoForge installer and isn't runnable yet) - see Rust's
   * `Instance::launch_mode`. */
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
  "paper",
  "purpur",
  "spigot",
  "bukkit",
];

/** Server types ModpackPilot can install by itself from just a Minecraft
 * version (see Rust's `install_server_jar`). */
export const DIRECT_DOWNLOAD_LOADERS: ServerLoader[] = [
  "vanilla",
  "paper",
  "purpur",
  "spigot",
  "bukkit",
];

/** The subset with no download: SpigotMC's BuildTools compiles them, which
 * takes several minutes and so runs from the server's page, not the Create
 * dialog. */
export const BUILD_TOOLS_LOADERS: ServerLoader[] = ["spigot", "bukkit"];

export function isDirectDownloadLoader(loader: ServerLoader): boolean {
  return DIRECT_DOWNLOAD_LOADERS.includes(loader);
}

export function isBuildToolsLoader(loader: ServerLoader): boolean {
  return BUILD_TOOLS_LOADERS.includes(loader);
}

/** Mirrors Rust's `LaunchHistoryEntry`. */
export interface LaunchHistoryEntry {
  id: string;
  startedAt: string;
  /** Null for the run that is still going. */
  stoppedAt: string | null;
  /** Shown rather than reduced to "crashed": 1 is the JVM dying on its own,
   * 137 on Linux is the OS out-of-memory killer, which points elsewhere. */
  exitCode: number | null;
  status: "running" | "stopped" | "crashed";
}
