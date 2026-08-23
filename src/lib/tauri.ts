import { invoke } from "@tauri-apps/api/core";
import type { CreateInstanceRequest, Instance } from "@/types/instance";
import type {
  DetectedServerInfo,
  ImportInstanceRequest,
  ImportSource,
} from "@/types/import";
import type { JavaInstallation } from "@/types/java";

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

  analyzeImport: (source: ImportSource) =>
    invoke<DetectedServerInfo>("analyze_import", { source }),

  importInstance: (source: ImportSource, request: ImportInstanceRequest) =>
    invoke<Instance>("import_instance", { source, request }),

  setInstanceJava: (id: string, javaInstallationId: string | null) =>
    invoke<Instance>("set_instance_java", { id, javaInstallationId }),

  listJavaInstallations: () => invoke<JavaInstallation[]>("list_java_installations"),

  detectJavaInstallations: () => invoke<JavaInstallation[]>("detect_java_installations"),

  setDefaultJava: (id: string) => invoke<void>("set_default_java", { id }),

  startInstance: (id: string) => invoke<void>("start_instance", { id }),

  stopInstance: (id: string) => invoke<void>("stop_instance", { id }),

  forceStopInstance: (id: string) => invoke<void>("force_stop_instance", { id }),

  restartInstance: (id: string) => invoke<void>("restart_instance", { id }),

  sendConsoleCommand: (id: string, command: string) =>
    invoke<void>("send_console_command", { id, command }),
};
