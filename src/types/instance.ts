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
  jvmArgs: string[];
  serverArgs: string[];
  status: ServerStatus;
  autoStart: boolean;
  autoRestart: boolean;
  createdAt: string;
  lastLaunchedAt: string | null;
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
