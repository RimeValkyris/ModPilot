import type { ServerLoader } from "@/types/instance";

/** Mirrors Rust's `ModrinthSearchHit`. */
export interface ModrinthSearchHit {
  projectId: string;
  slug: string;
  title: string;
  description: string;
  iconUrl: string | null;
}

/** Mirrors Rust's `ModrinthVersion` (the `files` field is server-only). */
export interface ModrinthVersion {
  id: string;
  name: string;
  versionNumber: string;
  changelog: string | null;
  datePublished: string;
  loaders: string[];
  gameVersions: string[];
}

/** Mirrors Rust's `ModrinthVersionPreview` - what installing a version
 * would do, derived from Modrinth's version metadata.
 *
 * Deliberately thinner than `FtbVersionPreview`: file count, mod count and
 * download size live inside the `.mrpack`, which can be hundreds of
 * megabytes, so they are reported during the install rather than before it.
 */
export interface ModrinthVersionPreview {
  versionId: string;
  versionName: string;
  minecraftVersion: string | null;
  loader: ServerLoader;
  warnings: string[];
}

/** Mirrors Rust's `ModrinthImportRequest`. */
export interface ModrinthImportRequest {
  projectId: string;
  versionId: string;
  name: string;
  minRamMb?: number;
  maxRamMb?: number;
  overwrite: boolean;
}

/** Mirrors Rust's `ModpackUpdateCheck`. `latestVersion` is `null` when no
 * published version matches the instance's loader/Minecraft version. */
export interface ModpackUpdateCheck {
  hasUpdate: boolean;
  currentVersionId: string | null;
  latestVersion: ModrinthVersion | null;
}
