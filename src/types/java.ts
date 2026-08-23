/** Mirrors Rust's `JavaInstallation`. */
export interface JavaInstallation {
  id: string;
  version: string;
  vendor: string | null;
  path: string;
  architecture: string;
  isDefault: boolean;
  detectedAt: string;
}
