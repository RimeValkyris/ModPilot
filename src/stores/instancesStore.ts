import { create } from "zustand";
import { api } from "@/lib/tauri";
import type {
  CreateInstanceRequest,
  Instance,
  ServerStatus,
  UpdateInstanceSettingsRequest,
} from "@/types/instance";
import type { ImportInstanceRequest, ImportSource } from "@/types/import";

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
  renameInstance: (id: string, newName: string) => Promise<Instance>;
  duplicateInstance: (id: string, newName: string) => Promise<Instance>;
  setInstanceJava: (id: string, javaInstallationId: string | null) => Promise<Instance>;
  updateInstanceSettings: (
    id: string,
    request: UpdateInstanceSettingsRequest,
  ) => Promise<Instance>;
  deleteInstance: (id: string) => Promise<void>;
  linkModrinthProject: (id: string, projectId: string) => Promise<Instance>;
  unlinkModrinthProject: (id: string) => Promise<Instance>;
  applyModpackUpdate: (id: string, versionId: string) => Promise<Instance>;
  installForgeServer: (id: string) => Promise<Instance>;
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

  applyStatusUpdate: (id, status) => {
    set({
      instances: get().instances.map((i) => (i.id === id ? { ...i, status } : i)),
    });
    get().clearInstanceStuck(id);
  },
}));
