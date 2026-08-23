import { create } from "zustand";
import { api } from "@/lib/tauri";
import type { CreateInstanceRequest, Instance, ServerStatus } from "@/types/instance";
import type { ImportInstanceRequest, ImportSource } from "@/types/import";

interface InstancesState {
  instances: Instance[];
  isLoading: boolean;
  error: string | null;
  fetchInstances: () => Promise<void>;
  createInstance: (request: CreateInstanceRequest) => Promise<Instance>;
  importInstance: (
    source: ImportSource,
    request: ImportInstanceRequest,
  ) => Promise<Instance>;
  renameInstance: (id: string, newName: string) => Promise<Instance>;
  setInstanceJava: (id: string, javaInstallationId: string | null) => Promise<Instance>;
  deleteInstance: (id: string) => Promise<void>;
  /** Applied from the `instance-status-changed` Tauri event. */
  applyStatusUpdate: (id: string, status: ServerStatus) => void;
}

export const useInstancesStore = create<InstancesState>((set, get) => ({
  instances: [],
  isLoading: false,
  error: null,

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

  deleteInstance: async (id) => {
    await api.deleteInstance(id);
    set({ instances: get().instances.filter((i) => i.id !== id) });
  },

  applyStatusUpdate: (id, status) => {
    set({
      instances: get().instances.map((i) => (i.id === id ? { ...i, status } : i)),
    });
  },
}));
