export const THEMES = ["light", "dark", "dracula", "nord"] as const;
export type Theme = (typeof THEMES)[number];

export const THEME_LABELS: Record<Theme, string> = {
  light: "Light",
  dark: "Dark",
  dracula: "Dracula",
  nord: "Nord",
};

/** Swatch colors for the theme picker preview - matches index.css. */
export const THEME_SWATCHES: Record<Theme, { background: string; accent: string }> = {
  light: { background: "#ffffff", accent: "#171717" },
  dark: { background: "#252525", accent: "#e5e5e5" },
  dracula: { background: "#282a36", accent: "#bd93f9" },
  nord: { background: "#2e3440", accent: "#88c0d0" },
};

const HTML_CLASSES: Record<Theme, string> = {
  light: "",
  dark: "dark",
  dracula: "dark theme-dracula",
  nord: "dark theme-nord",
};

export function applyThemeClass(theme: Theme) {
  document.documentElement.className = HTML_CLASSES[theme];
}
