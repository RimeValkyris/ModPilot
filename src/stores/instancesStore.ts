import { create } from "zustand";
import { api } from "@/lib/tauri";
import type { CreateInstanceRequest, Instance } from "@/types/instance";

interface InstancesState {
  instances: Instance[];
  isLoading: boolean;
  error: string | null;
  fetchInstances: () => Promise<void>;
  createInstance: (request: CreateInstanceRequest) => Promise<Instance>;
  renameInstance: (id: string, newName: string) => Promise<Instance>;
  deleteInstance: (id: string) => Promise<void>;
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

  renameInstance: async (id, newName) => {
    const updated = await api.renameInstance(id, newName);
    set({
      instances: get().instances.map((i) => (i.id === id ? updated : i)),
    });
    return updated;
  },

  deleteInstance: async (id) => {
    await api.deleteInstance(id);
    set({ instances: get().instances.filter((i) => i.id !== id) });
  },
}));
