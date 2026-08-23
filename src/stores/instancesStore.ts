import { create } from "zustand";
import { api } from "@/lib/tauri";
import type { Instance } from "@/types/instance";

interface InstancesState {
  instances: Instance[];
  isLoading: boolean;
  error: string | null;
  fetchInstances: () => Promise<void>;
}

export const useInstancesStore = create<InstancesState>((set) => ({
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
}));
