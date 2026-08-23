import { invoke } from "@tauri-apps/api/core";
import type { CreateInstanceRequest, Instance } from "@/types/instance";

/**
 * Thin wrapper around Tauri's `invoke` calls. Keeping every command call in
 * one place means the frontend never constructs Rust-side behavior itself -
 * it just asks for data and renders it.
 */
export const api = {
  listInstances: () => invoke<Instance[]>("list_instances"),

  createInstance: (request: CreateInstanceRequest) =>
    invoke<Instance>("create_instance", { request }),

  renameInstance: (id: string, newName: string) =>
    invoke<Instance>("rename_instance", { id, newName }),

  deleteInstance: (id: string) => invoke<void>("delete_instance", { id }),
};
