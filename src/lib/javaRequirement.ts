/**
 * Minecraft's official minimum Java version per release, per Mojang's own
 * launcher requirements. Ordered newest-first; the first threshold the
 * given version meets or exceeds wins.
 */
const THRESHOLDS: Array<{ min: [number, number, number]; java: number }> = [
  { min: [1, 20, 5], java: 21 },
  { min: [1, 18, 0], java: 17 },
  { min: [1, 17, 0], java: 16 },
  { min: [0, 0, 0], java: 8 },
];

function parseVersionTuple(v: string): [number, number, number] | null {
  const match = v.trim().match(/^(\d+)(?:\.(\d+))?(?:\.(\d+))?/);
  if (!match) return null;
  return [Number(match[1]), Number(match[2] ?? 0), Number(match[3] ?? 0)];
}

function compareTuples(a: [number, number, number], b: [number, number, number]): number {
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i]) return a[i] - b[i];
  }
  return 0;
}

/**
 * Returns the Java major version a given Minecraft release needs (per
 * Mojang's own requirements), or `null` if the version string can't be
 * parsed (snapshots, unknown, etc.) - never guessed with false confidence.
 */
export function getRequiredJavaMajor(minecraftVersion: string | null | undefined): number | null {
  if (!minecraftVersion) return null;
  const tuple = parseVersionTuple(minecraftVersion);
  if (!tuple) return null;

  for (const threshold of THRESHOLDS) {
    if (compareTuples(tuple, threshold.min) >= 0) return threshold.java;
  }
  return 8;
}

/** Parses the major version out of a JavaInstallation's normalized version string ("21.0.2" -> 21). */
export function parseJavaMajor(javaVersion: string): number | null {
  const major = parseInt(javaVersion.split(".")[0] ?? "", 10);
  return Number.isNaN(major) ? null : major;
}
