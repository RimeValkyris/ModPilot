import { create } from "zustand";
import { api } from "@/lib/tauri";
import type { JavaInstallation } from "@/types/java";

interface JavaState {
  installations: JavaInstallation[];
  isLoading: boolean;
  isScanning: boolean;
  error: string | null;
  /** Reads whatever is already known, without rescanning the system. */
  fetchInstallations: () => Promise<void>;
  /** Rescans the system (`java -version` sweep) and refreshes the list. */
  rescan: () => Promise<void>;
  setDefault: (id: string) => Promise<void>;
}

export const useJavaStore = create<JavaState>((set, get) => ({
  installations: [],
  isLoading: false,
  isScanning: false,
  error: null,

  fetchInstallations: async () => {
    // Every InstanceCard calls this on mount, and a page can render many of
    // them in the same render pass - without this guard, N cards mounting
    // together fire N redundant `list_java_installations` IPC calls at
    // once. `isLoading` flips synchronously via zustand's `set`, so the
    // first call wins and the rest skip, all still landing in the same
    // shared store state.
    if (get().isLoading) return;
    set({ isLoading: true, error: null });
    try {
      const installations = await api.listJavaInstallations();
      set({ installations, isLoading: false });
    } catch (err) {
      set({ error: String(err), isLoading: false });
    }
  },

  rescan: async () => {
    set({ isScanning: true, error: null });
    try {
      const installations = await api.detectJavaInstallations();
      set({ installations, isScanning: false });
    } catch (err) {
      set({ error: String(err), isScanning: false });
    }
  },

  setDefault: async (id) => {
    await api.setDefaultJava(id);
    set({
      installations: get().installations.map((j) => ({
        ...j,
        isDefault: j.id === id,
      })),
    });
  },
}));
