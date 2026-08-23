import { create } from "zustand";
import { api } from "@/lib/tauri";

interface AppWallpaperState {
  wallpaper: string | null;
  blur: number;
  dim: number;
  isLoaded: boolean;
  load: () => Promise<void>;
  setWallpaper: (sourcePath: string) => Promise<void>;
  clearWallpaper: () => Promise<void>;
  setBlur: (blur: number) => Promise<void>;
  setDim: (dim: number) => Promise<void>;
}

export const useAppWallpaperStore = create<AppWallpaperState>((set) => ({
  wallpaper: null,
  blur: 0,
  dim: 40,
  isLoaded: false,

  load: async () => {
    try {
      const [wallpaper, blurSetting, dimSetting] = await Promise.all([
        api.readAppWallpaper(),
        api.getAppSetting("app_wallpaper_blur"),
        api.getAppSetting("app_wallpaper_dim"),
      ]);
      set({
        wallpaper,
        blur: blurSetting ? Number(blurSetting) : 0,
        dim: dimSetting ? Number(dimSetting) : 40,
        isLoaded: true,
      });
    } catch {
      set({ wallpaper: null, isLoaded: true });
    }
  },

  setWallpaper: async (sourcePath) => {
    await api.setAppWallpaper(sourcePath);
    set({ wallpaper: await api.readAppWallpaper() });
  },

  clearWallpaper: async () => {
    await api.clearAppWallpaper();
    set({ wallpaper: null });
  },

  setBlur: async (blur) => {
    set({ blur });
    await api.setAppSetting("app_wallpaper_blur", String(blur));
  },

  setDim: async (dim) => {
    set({ dim });
    await api.setAppSetting("app_wallpaper_dim", String(dim));
  },
}));
