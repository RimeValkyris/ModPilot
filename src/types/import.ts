import type { ServerLoader } from "@/types/instance";

/** Mirrors Rust's `ImportSource` (tagged by `kind`). */
export type ImportSource =
  | { kind: "zip"; path: string }
  | { kind: "folder"; path: string };

/** Mirrors Rust's `DetectedServerInfo`. */
export interface DetectedServerInfo {
  minecraftVersion: string | null;
  loader: ServerLoader;
  loaderVersion: string | null;
  serverJar: string | null;
  /** `serverJar` is a Forge/NeoForge `@`-argfile, not a runnable jar. */
  serverJarIsArgfile: boolean;
  /** `serverJar` is the pack's own start script, to be run as-is. */
  serverJarIsScript: boolean;
  /** The Forge/NeoForge installer jar the pack ships, if any. */
  loaderInstaller: string | null;
  /** The pack ships only an installer - it has to be installed once before
   * it can start, and `serverJar` is that installer. */
  needsLoaderInstall: boolean;
  hasModsFolder: boolean;
  modCount: number;
  hasConfigFolder: boolean;
  hasWorldFolder: boolean;
  worldFolderName: string | null;
  hasServerProperties: boolean;
  startScripts: string[];
  /** `-Xms`/`-Xmx` the pack sets for itself, in MB, if it says. These
   * pre-fill the import form's RAM fields. */
  suggestedMinRamMb: number | null;
  suggestedMaxRamMb: number | null;
  warnings: string[];
}

/** Mirrors Rust's `ImportInstanceRequest`. */
export interface ImportInstanceRequest {
  name: string;
  minecraftVersion?: string | null;
  loader?: ServerLoader | null;
  loaderVersion?: string | null;
  minRamMb?: number | null;
  maxRamMb?: number | null;
  overwrite?: boolean;
}

/** Matches the Rust command's marker prefix for a name collision. */
export const INSTANCE_EXISTS_PREFIX = "INSTANCE_EXISTS:";
