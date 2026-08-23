import { create } from "zustand";
import { api } from "@/lib/tauri";
import { applyThemeClass, THEMES, type Theme } from "@/types/theme";

const SETTING_KEY = "theme";
const DEFAULT_THEME: Theme = "dark";

interface ThemeState {
  theme: Theme;
  isLoaded: boolean;
  load: () => Promise<void>;
  setTheme: (theme: Theme) => Promise<void>;
}

export const useThemeStore = create<ThemeState>((set) => ({
  theme: DEFAULT_THEME,
  isLoaded: false,

  load: async () => {
    try {
      const stored = await api.getAppSetting(SETTING_KEY);
      const theme = THEMES.includes(stored as Theme) ? (stored as Theme) : DEFAULT_THEME;
      applyThemeClass(theme);
      set({ theme, isLoaded: true });
    } catch {
      applyThemeClass(DEFAULT_THEME);
      set({ theme: DEFAULT_THEME, isLoaded: true });
    }
  },

  setTheme: async (theme) => {
    applyThemeClass(theme);
    set({ theme });
    await api.setAppSetting(SETTING_KEY, theme);
  },
}));
