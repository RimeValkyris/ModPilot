import { create } from "zustand";
import { api } from "@/lib/tauri";

interface WallpaperState {
  /** `undefined` = not fetched yet, `null` = fetched, instance has none. */
  wallpapers: Record<string, string | null | undefined>;
  fetchWallpaper: (id: string) => Promise<void>;
  setWallpaper: (id: string, sourcePath: string) => Promise<void>;
  clearWallpaper: (id: string) => Promise<void>;
}

export const useWallpaperStore = create<WallpaperState>((set, get) => ({
  wallpapers: {},

  fetchWallpaper: async (id) => {
    if (get().wallpapers[id] !== undefined) return;
    try {
      const dataUri = await api.readInstanceWallpaper(id);
      set({ wallpapers: { ...get().wallpapers, [id]: dataUri } });
    } catch {
      set({ wallpapers: { ...get().wallpapers, [id]: null } });
    }
  },

  setWallpaper: async (id, sourcePath) => {
    await api.setInstanceWallpaper(id, sourcePath);
    const dataUri = await api.readInstanceWallpaper(id);
    set({ wallpapers: { ...get().wallpapers, [id]: dataUri } });
  },

  clearWallpaper: async (id) => {
    await api.clearInstanceWallpaper(id);
    set({ wallpapers: { ...get().wallpapers, [id]: null } });
  },
}));
