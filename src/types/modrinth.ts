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

/** Mirrors Rust's `ModpackUpdateCheck`. `latestVersion` is `null` when no
 * published version matches the instance's loader/Minecraft version. */
export interface ModpackUpdateCheck {
  hasUpdate: boolean;
  currentVersionId: string | null;
  latestVersion: ModrinthVersion | null;
}
