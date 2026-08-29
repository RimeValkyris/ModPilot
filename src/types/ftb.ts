import type { ServerLoader } from "@/types/instance";

/** Mirrors Rust's `FtbTarget` - what a version runs on. */
export interface FtbTarget {
  name: string;
  version: string;
  type: string;
}

/** Mirrors Rust's `FtbVersionSummary`. `updated` is Unix seconds, and
 * `targets` is why picking a compatible version costs no extra requests. */
export interface FtbVersionSummary {
  id: number;
  name: string;
  type: string;
  updated: number;
  targets: FtbTarget[];
}

/** Mirrors Rust's `FtbUpdateCheck`. `latestVersion` is null when the pack
 * has versions but none match this instance's loader/Minecraft version. */
export interface FtbUpdateCheck {
  hasUpdate: boolean;
  currentVersionId: number | null;
  latestVersion: FtbVersionSummary | null;
}

/** Mirrors Rust's `FtbPack`. `iconUrl` is picked out of FTB's `art` list
 * by the backend, so the UI doesn't need to know its art-type vocabulary. */
export interface FtbPack {
  id: number;
  name: string;
  slug: string;
  synopsis: string;
  versions: FtbVersionSummary[];
  iconUrl: string | null;
}

/** Mirrors Rust's `FtbImportRequest`. */
export interface FtbImportRequest {
  packId: number;
  versionId: number;
  name: string;
  minRamMb?: number;
  maxRamMb?: number;
  overwrite: boolean;
}

/** Mirrors Rust's `FtbVersionPreview` - what installing a version would do,
 * derived from the manifest without downloading anything. */
export interface FtbVersionPreview {
  versionId: number;
  versionName: string;
  minecraftVersion: string | null;
  loader: ServerLoader;
  loaderVersion: string | null;
  javaMajor: number | null;
  modCount: number;
  totalFiles: number;
  downloadSizeBytes: number;
  warnings: string[];
}

/** Mirrors Rust's `FtbInstallProgressPayload` (event
 * `ftb-install-progress`). */
export interface FtbInstallProgress {
  phase: "downloading" | "installing-loader" | "done";
  done: number;
  total: number;
  detail: string | null;
}

export const FTB_INSTALL_PROGRESS_EVENT = "ftb-install-progress";
