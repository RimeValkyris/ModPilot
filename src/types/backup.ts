/** Mirrors Rust's `WorldBackup`. */
export interface WorldBackup {
  name: string;
  sizeBytes: number;
  createdAt: string;
}

/** Mirrors Rust's `BackupVerification` - what reading a backup all the way
 * through actually found in it. */
export interface BackupVerification {
  name: string;
  fileCount: number;
  uncompressedBytes: number;
}

/** Mirrors Rust's `RestoreOutcome`. `displacedWorld` names the folder the
 * previous world was moved aside to, so the UI can tell the operator where
 * their old save went rather than implying it was destroyed. */
export interface RestoreOutcome {
  restoredFrom: string;
  displacedWorld: string | null;
  fileCount: number;
}
