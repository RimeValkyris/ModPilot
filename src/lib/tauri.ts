import { invoke } from "@tauri-apps/api/core";
import type { Instance } from "@/types/instance";

/**
 * Thin wrapper around Tauri's `invoke` calls. Keeping every command call in
 * one place means the frontend never constructs Rust-side behavior itself -
 * it just asks for data and renders it.
 */
export const api = {
  listInstances: () => invoke<Instance[]>("list_instances"),
};
