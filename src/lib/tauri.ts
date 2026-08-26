import { invoke } from "@tauri-apps/api/core";
import type {
  CreateInstanceRequest,
  Instance,
  UpdateInstanceSettingsRequest,
} from "@/types/instance";
import type {
  DetectedServerInfo,
  ImportInstanceRequest,
  ImportSource,
} from "@/types/import";
import type { JavaInstallation } from "@/types/java";
import type { ResourceUsage } from "@/types/monitor";
import type { WorldBackup } from "@/types/backup";
import type { ModInfo } from "@/types/mod";
import type { ModpackUpdateCheck, ModrinthSearchHit, ModrinthVersion } from "@/types/modrinth";
import type { AvatarPresetInfo } from "@/types/avatar";

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

  readLatestLog: (id: string) => invoke<string>("read_latest_log", { id }),

  listServerJars: (id: string) => invoke<string[]>("list_server_jars", { id }),

  updateInstanceSettings: (id: string, request: UpdateInstanceSettingsRequest) =>
    invoke<Instance>("update_instance_settings", { id, request }),

  getResourceUsage: (id: string) => invoke<ResourceUsage>("get_resource_usage", { id }),

  getAllResourceUsage: () => invoke<Record<string, ResourceUsage>>("get_all_resource_usage"),

  getSystemMemoryMb: () => invoke<number>("get_system_memory_mb"),

  listOnlinePlayers: (id: string) => invoke<string[]>("list_online_players", { id }),

  setInstanceSchedules: (
    id: string,
    restartSchedule: string | null,
    backupSchedule: string | null,
    backupKeepLast: number,
  ) =>
    invoke<Instance>("set_instance_schedules", {
      id,
      restartSchedule,
      backupSchedule,
      backupKeepLast,
    }),

  listRunningInstanceIds: () => invoke<string[]>("list_running_instance_ids"),

  getAppLogsDir: () => invoke<string>("get_app_logs_dir"),

  /** Opens one of ModpackPilot's own folders. The backend resolves the path
   * itself - the frontend never names a path to open, which is what lets the
   * opener plugin stay scoped to nothing. */
  openManagedFolder: (
    target:
      | { appLogs: Record<string, never> }
      | { instanceLogs: { id: string } }
      | { instanceSubfolder: { id: string; folder: string } },
  ) => invoke<void>("open_managed_folder", { target }),

  exportAppLog: (destPath: string) => invoke<void>("export_app_log", { destPath }),

  exportInstanceLog: (id: string, destPath: string) =>
    invoke<void>("export_instance_log", { id, destPath }),

  getInstanceLogsDir: (id: string) => invoke<string>("get_instance_logs_dir", { id }),

  duplicateInstance: (id: string, newName: string) =>
    invoke<Instance>("duplicate_instance", { id, newName }),

  getInstanceSubfolder: (id: string, folder: "server" | "mods" | "config" | "world") =>
    invoke<string>("get_instance_subfolder", { id, folder }),

  createWorldBackup: (id: string) => invoke<string>("create_world_backup", { id }),

  listWorldBackups: (id: string) => invoke<WorldBackup[]>("list_world_backups", { id }),

  restoreWorldBackup: (id: string, backupName: string) =>
    invoke<void>("restore_world_backup", { id, backupName }),

  deleteWorldBackup: (id: string, backupName: string) =>
    invoke<void>("delete_world_backup", { id, backupName }),

  readPlayerList: (id: string, file: string) =>
    invoke<unknown[]>("read_player_list", { id, file }),

  writePlayerList: (id: string, file: string, entries: unknown[]) =>
    invoke<void>("write_player_list", { id, file, entries }),

  listMods: (id: string) => invoke<ModInfo[]>("list_mods", { id }),

  toggleMod: (id: string, fileName: string) => invoke<void>("toggle_mod", { id, fileName }),

  deleteMod: (id: string, fileName: string) => invoke<void>("delete_mod", { id, fileName }),

  getAppSetting: (key: string) => invoke<string | null>("get_app_setting", { key }),

  setAppSetting: (key: string, value: string) => invoke<void>("set_app_setting", { key, value }),

  getInstancesDir: () => invoke<string>("get_instances_dir"),

  getPortableInstancesDir: () => invoke<string>("get_portable_instances_dir"),

  setInstancesDir: (newDir: string, moveExisting: boolean) =>
    invoke<void>("set_instances_dir", { newDir, moveExisting }),

  quitApp: () => invoke<void>("quit_app"),

  resetJavaInstallations: () => invoke<void>("reset_java_installations"),

  setServerIcon: (id: string, sourcePath: string) =>
    invoke<void>("set_server_icon", { id, sourcePath }),

  clearServerIcon: (id: string) => invoke<void>("clear_server_icon", { id }),

  readServerIcon: (id: string) => invoke<string | null>("read_server_icon", { id }),

  readServerProperties: (id: string) =>
    invoke<Record<string, string>>("read_server_properties", { id }),

  writeServerProperties: (id: string, updates: Record<string, string>) =>
    invoke<void>("write_server_properties", { id, updates }),

  setInstanceAvatar: (id: string, sourcePath: string) =>
    invoke<void>("set_instance_avatar", { id, sourcePath }),

  clearInstanceAvatar: (id: string) => invoke<void>("clear_instance_avatar", { id }),

  readInstanceAvatar: (id: string) => invoke<string | null>("read_instance_avatar", { id }),

  listAvatarPresets: () => invoke<AvatarPresetInfo[]>("list_avatar_presets"),

  setInstanceAvatarPreset: (id: string, presetId: string) =>
    invoke<void>("set_instance_avatar_preset", { id, presetId }),

  searchModrinthProjects: (query: string) =>
    invoke<ModrinthSearchHit[]>("search_modrinth_projects", { query }),

  linkModrinthProject: (id: string, projectId: string) =>
    invoke<Instance>("link_modrinth_project", { id, projectId }),

  unlinkModrinthProject: (id: string) => invoke<Instance>("unlink_modrinth_project", { id }),

  checkModpackUpdate: (id: string) =>
    invoke<ModpackUpdateCheck>("check_modpack_update", { id }),

  listModpackVersions: (id: string) =>
    invoke<ModrinthVersion[]>("list_modpack_versions", { id }),

  applyModpackUpdate: (id: string, versionId: string) =>
    invoke<Instance>("apply_modpack_update", { id, versionId }),

  installForgeServer: (id: string) => invoke<Instance>("install_forge_server", { id }),
};
