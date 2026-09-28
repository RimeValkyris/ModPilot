export const THEMES = ["dark", "light", "minecraft", "dracula", "nord"] as const;
export type Theme = (typeof THEMES)[number];

export const THEME_LABELS: Record<Theme, string> = {
  light: "Soft Light",
  dark: "Soft Dark",
  minecraft: "Minecraft",
  dracula: "Dracula",
  nord: "Nord",
};

/** Swatch colors for the theme picker preview - matches index.css. */
export const THEME_SWATCHES: Record<Theme, { background: string; accent: string }> = {
  light: { background: "#e4e8ee", accent: "#2f9e72" },
  dark: { background: "#2a2d33", accent: "#5fd3a0" },
  minecraft: { background: "#25252b", accent: "#6aa84f" },
  dracula: { background: "#282a36", accent: "#bd93f9" },
  nord: { background: "#2e3440", accent: "#88c0d0" },
};

const HTML_CLASSES: Record<Theme, string> = {
  // The two core themes are neumorphic: surfaces share the page color and
  // are shaped by paired light/dark shadows (see `.theme-neu` in index.css).
  light: "theme-neu",
  dark: "dark theme-neu",
  // Rides on `dark` so every `dark:` variant in the app still applies -
  // the Minecraft theme is a dark theme with its own palette, blocky
  // geometry, and beveled panels layered on top.
  minecraft: "dark theme-minecraft",
  dracula: "dark theme-dracula",
  nord: "dark theme-nord",
};

export function applyThemeClass(theme: Theme) {
  document.documentElement.className = HTML_CLASSES[theme];
}
