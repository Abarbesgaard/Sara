// Deterministic colour per project name, so the same project keeps its hue
// across renders. Memories with no project fall back to a neutral grey.
const NEUTRAL = "#8a8f98";
const NEUTRAL_BIOLUM = "#4a6472";

// A curated bioluminescent palette — cyans, teals, greens, violets and magentas
// on near-black. Perceptually cohesive (unlike the full-360° hash hue), so the
// cloud reads as one organism lit from within rather than a bag of clashing
// crayons. Indexed deterministically by project name.
const BIOLUM = [
  "#00e5ff", // cyan
  "#18ffb2", // aqua-green
  "#5ffbf1", // pale teal
  "#4dd0ff", // sky
  "#7c5cff", // violet
  "#b388ff", // lavender
  "#ff5ec7", // magenta
  "#c774ff", // orchid
  "#39ff14", // neon green
  "#66ffd9", // mint
  "#ff8ae2", // pink
  "#5691ff", // periwinkle
];

function hashHue(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) % 360;
  return h;
}

function hashInt(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) >>> 0;
  return h;
}

export function projectColor(project: string | undefined, biolum = false): string {
  if (!project) return biolum ? NEUTRAL_BIOLUM : NEUTRAL;
  if (biolum) return BIOLUM[hashInt(project) % BIOLUM.length];
  return `hsl(${hashHue(project)}, 65%, 60%)`;
}

/** The colour used for a node: its first (alphabetical) project's hue. */
export function nodeColor(projects: string[], biolum = false): string {
  return projectColor(projects[0], biolum);
}
