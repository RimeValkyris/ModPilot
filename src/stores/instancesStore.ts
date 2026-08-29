import { create } from "zustand";
import { api } from "@/lib/tauri";
import type {
  CreateInstanceRequest,
  Instance,
  ServerStatus,
  UpdateInstanceSettingsRequest,
} from "@/types/instance";
import type { ImportInstanceRequest, ImportSource } from "@/types/import";
import type { FtbImportRequest } from "@/types/ftb";

interface InstancesState {
  instances: Instance[];
  isLoading: boolean;
  error: string | null;
  /** Ids currently flagged by the backend's startup watchdog as having
   * gone quiet for too long while starting - see `instance-stuck-starting`.
   * Global (not scoped to whichever console tab happens to be open), and
   * cleared as soon as the instance produces new output or leaves the
   * "starting" state. */
  stuckInstanceIds: Set<string>;
  markInstanceStuck: (id: string) => void;
  clearInstanceStuck: (id: string) => void;
  fetchInstances: () => Promise<void>;
  createInstance: (request: CreateInstanceRequest) => Promise<Instance>;
  importInstance: (
    source: ImportSource,
    request: ImportInstanceRequest,
  ) => Promise<Instance>;
  /** Installs an FTB modpack version as a new instance. Separate from
   * `importInstance` because there is no local source to copy - the files
   * are downloaded from FTB and the loader is installed afterwards. */
  importFtbInstance: (request: FtbImportRequest) => Promise<Instance>;
  renameInstance: (id: string, newName: string) => Promise<Instance>;
  duplicateInstance: (id: string, newName: string) => Promise<Instance>;
  setInstanceJava: (id: string, javaInstallationId: string | null) => Promise<Instance>;
  updateInstanceSettings: (
    id: string,
    request: UpdateInstanceSettingsRequest,
  ) => Promise<Instance>;
  deleteInstance: (id: string) => Promise<void>;
  linkFtbPack: (id: string, packId: number) => Promise<Instance>;
  unlinkFtbPack: (id: string) => Promise<Instance>;
  applyFtbUpdate: (id: string, versionId: number) => Promise<Instance>;
  linkModrinthProject: (id: string, projectId: string) => Promise<Instance>;
  unlinkModrinthProject: (id: string) => Promise<Instance>;
  applyModpackUpdate: (id: string, versionId: string) => Promise<Instance>;
  installForgeServer: (id: string) => Promise<Instance>;
  setInstanceSchedules: (
    id: string,
    restartSchedule: string | null,
    backupSchedule: string | null,
    backupKeepLast: number,
  ) => Promise<Instance>;
  setUpdatePolicy: (id: string, policy: string) => Promise<Instance>;
  updateInstanceFromSource: (id: string, source: ImportSource) => Promise<Instance>;
  /** Applied from the `instance-status-changed` Tauri event. */
  applyStatusUpdate: (id: string, status: ServerStatus) => void;
}

export const useInstancesStore = create<InstancesState>((set, get) => ({
  instances: [],
  isLoading: false,
  error: null,
  stuckInstanceIds: new Set(),

  markInstanceStuck: (id) => {
    set({ stuckInstanceIds: new Set(get().stuckInstanceIds).add(id) });
  },

  clearInstanceStuck: (id) => {
    if (!get().stuckInstanceIds.has(id)) return;
    const next = new Set(get().stuckInstanceIds);
    next.delete(id);
    set({ stuckInstanceIds: next });
  },

  fetchInstances: async () => {
    set({ isLoading: true, error: null });
    try {
      const instances = await api.listInstances();
      set({ instances, isLoading: false });
    } catch (err) {
      set({ error: String(err), isLoading: false });
    }
  },

  createInstance: async (request) => {
    const instance = await api.createInstance(request);
    set({ instances: [instance, ...get().instances] });
    return instance;
  },

  importInstance: async (source, request) => {
    const instance = await api.importInstance(source, request);
    set({ instances: [instance, ...get().instances] });
    return instance;
  },

  importFtbInstance: async (request) => {
    const instance = await api.importFtbInstance(request);
    set({ instances: [instance, ...get().instances] });
    return instance;
  },

  renameInstance: async (id, newName) => {
    const updated = await api.renameInstance(id, newName);
    set({
      instances: get().instances.map((i) => (i.id === id ? updated : i)),
    });
    return updated;
  },

  setInstanceJava: async (id, javaInstallationId) => {
    const updated = await api.setInstanceJava(id, javaInstallationId);
    set({
      instances: get().instances.map((i) => (i.id === id ? updated : i)),
    });
    return updated;
  },

  duplicateInstance: async (id, newName) => {
    const instance = await api.duplicateInstance(id, newName);
    set({ instances: [instance, ...get().instances] });
    return instance;
  },

  updateInstanceSettings: async (id, request) => {
    const updated = await api.updateInstanceSettings(id, request);
    set({
      instances: get().instances.map((i) => (i.id === id ? updated : i)),
    });
    return updated;
  },

  deleteInstance: async (id) => {
    await api.deleteInstance(id);
    set({ instances: get().instances.filter((i) => i.id !== id) });
  },

  linkFtbPack: async (id, packId) => {
    const updated = await api.linkFtbPack(id, packId);
    set({ instances: get().instances.map((i) => (i.id === id ? updated : i)) });
    return updated;
  },

  unlinkFtbPack: async (id) => {
    const updated = await api.unlinkFtbPack(id);
    set({ instances: get().instances.map((i) => (i.id === id ? updated : i)) });
    return updated;
  },

  applyFtbUpdate: async (id, versionId) => {
    const updated = await api.applyFtbUpdate(id, versionId);
    set({ instances: get().instances.map((i) => (i.id === id ? updated : i)) });
    return updated;
  },

  linkModrinthProject: async (id, projectId) => {
    const updated = await api.linkModrinthProject(id, projectId);
    set({ instances: get().instances.map((i) => (i.id === id ? updated : i)) });
    return updated;
  },

  unlinkModrinthProject: async (id) => {
    const updated = await api.unlinkModrinthProject(id);
    set({ instances: get().instances.map((i) => (i.id === id ? updated : i)) });
    return updated;
  },

  applyModpackUpdate: async (id, versionId) => {
    const updated = await api.applyModpackUpdate(id, versionId);
    set({ instances: get().instances.map((i) => (i.id === id ? updated : i)) });
    return updated;
  },

  installForgeServer: async (id) => {
    const updated = await api.installForgeServer(id);
    set({ instances: get().instances.map((i) => (i.id === id ? updated : i)) });
    return updated;
  },

  setInstanceSchedules: async (id, restartSchedule, backupSchedule, backupKeepLast) => {
    const updated = await api.setInstanceSchedules(
      id,
      restartSchedule,
      backupSchedule,
      backupKeepLast,
    );
    set({ instances: get().instances.map((i) => (i.id === id ? updated : i)) });
    return updated;
  },

  setUpdatePolicy: async (id, policy) => {
    const updated = await api.setUpdatePolicy(id, policy);
    set({ instances: get().instances.map((i) => (i.id === id ? updated : i)) });
    return updated;
  },

  updateInstanceFromSource: async (id, source) => {
    const updated = await api.updateInstanceFromSource(id, source);
    set({ instances: get().instances.map((i) => (i.id === id ? updated : i)) });
    return updated;
  },

  applyStatusUpdate: (id, status) => {
    set({
      instances: get().instances.map((i) => (i.id === id ? { ...i, status } : i)),
    });
    get().clearInstanceStuck(id);
  },
}));
