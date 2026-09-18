// Deterministic colour per project name, so the same project keeps its hue
// across renders. Memories with no project fall back to a neutral grey.
const NEUTRAL = "#8a8f98";

function hashHue(s: string): number {
  let h = 0;
  for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) % 360;
  return h;
}

export function projectColor(project: string | undefined): string {
  if (!project) return NEUTRAL;
  return `hsl(${hashHue(project)}, 65%, 60%)`;
}

/** The colour used for a node: its first (alphabetical) project's hue. */
export function nodeColor(projects: string[]): string {
  return projectColor(projects[0]);
}
