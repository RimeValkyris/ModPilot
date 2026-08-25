export const THEMES = ["minecraft", "light", "dark", "dracula", "nord"] as const;
export type Theme = (typeof THEMES)[number];

export const THEME_LABELS: Record<Theme, string> = {
  minecraft: "Minecraft",
  light: "Light",
  dark: "Dark",
  dracula: "Dracula",
  nord: "Nord",
};

/** Swatch colors for the theme picker preview - matches index.css. */
export const THEME_SWATCHES: Record<Theme, { background: string; accent: string }> = {
  minecraft: { background: "#25252b", accent: "#6aa84f" },
  light: { background: "#ffffff", accent: "#171717" },
  dark: { background: "#252525", accent: "#e5e5e5" },
  dracula: { background: "#282a36", accent: "#bd93f9" },
  nord: { background: "#2e3440", accent: "#88c0d0" },
};

const HTML_CLASSES: Record<Theme, string> = {
  // Rides on `dark` so every `dark:` variant in the app still applies -
  // the Minecraft theme is a dark theme with its own palette, blocky
  // geometry, and beveled panels layered on top.
  minecraft: "dark theme-minecraft",
  light: "",
  dark: "dark",
  dracula: "dark theme-dracula",
  nord: "dark theme-nord",
};

export function applyThemeClass(theme: Theme) {
  document.documentElement.className = HTML_CLASSES[theme];
}
