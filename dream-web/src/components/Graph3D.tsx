import { useEffect, useMemo, useRef } from "react";
import ForceGraph3D, { type ForceGraphMethods } from "react-force-graph-3d";
import SpriteText from "three-spritetext";
import type { Graph, GraphNode } from "../api.ts";
import { nodeColor } from "../color.ts";

interface Props {
  graph: Graph;
  matches: (n: GraphNode) => boolean; // true when the node passes the active filters
  filterSig: string; // changes whenever the active filter set changes
  filtersActive: boolean;
  onSelect: (n: GraphNode) => void;
  focusLabel: string | null; // fly-to target from the search box
}

// react-force-graph mutates node objects with x/y/z; keep our fields alongside.
type FGNode = GraphNode & { x?: number; y?: number; z?: number };

const DIM = "#23262b";

function desaturate(hsl: string): string {
  // lower the saturation/lightness of an hsl() colour for provisional nodes
  return hsl.replace(/hsl\((\d+),\s*\d+%,\s*\d+%\)/, "hsl($1, 30%, 38%)");
}

export function Graph3D({ graph, matches, filterSig, filtersActive, onSelect, focusLabel }: Props) {
  const fgRef = useRef<ForceGraphMethods<FGNode> | undefined>(undefined);

  // Reuse the same node object references so react-force-graph keeps positions;
  // re-memo on filterSig so the colour/label accessors re-run when filters change.
  const data = useMemo(
    () => ({
      nodes: graph.nodes as FGNode[],
      links: graph.edges.map((e) => ({ ...e })),
    }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [graph, filterSig],
  );

  const colorFor = (n: FGNode): string => {
    if (filtersActive && !matches(n)) return DIM;
    const base = nodeColor(n.projects);
    return n.provisional ? desaturate(base) : base;
  };

  // Fly the camera to a searched node.
  useEffect(() => {
    if (!focusLabel || !fgRef.current) return;
    const n = data.nodes.find((x) => x.label === focusLabel);
    if (n == null || n.x == null) return;
    flyTo(fgRef.current, n, 100);
  }, [focusLabel, data]);

  return (
    <ForceGraph3D<FGNode>
      ref={fgRef}
      graphData={data}
      backgroundColor="#0b0d10"
      showNavInfo={false}
      nodeVal={(n) => Math.max(1, (n.strength - 0.9) * 6)}
      nodeColor={(n) => colorFor(n)}
      nodeOpacity={0.92}
      nodeResolution={12}
      nodeLabel={(n) =>
        `<div class="tt"><b>${n.label}</b> · ${escapeHtml(n.title)}` +
        `<br/><span class="tt-tags">${n.tags.map(escapeHtml).join(", ") || "—"}</span>` +
        `<br/><span class="tt-meta">${n.projects.map(escapeHtml).join(", ") || "no project"}` +
        `${n.canonical ? ` · canonical (${n.derivedCount})` : ""}` +
        `${n.provisional ? " · provisional" : ""}</span></div>`
      }
      nodeThreeObjectExtend={true}
      nodeThreeObject={(n) => {
        // Canonical badge: a small floating "⬡N" sprite above the sphere.
        if (!n.canonical || n.derivedCount <= 0) return undefined as unknown as never;
        if (filtersActive && !matches(n)) return undefined as unknown as never;
        const sprite = new SpriteText(`⬡${n.derivedCount}`);
        sprite.color = "#ffd76a";
        sprite.textHeight = 4;
        sprite.position.set(0, Math.max(3, (n.strength - 0.9) * 6) + 3, 0);
        return sprite;
      }}
      linkColor={(l) => ((l as unknown as { kind: string }).kind === "bond" ? "#6ea8fe" : "#333941")}
      linkWidth={(l) => ((l as unknown as { kind: string }).kind === "bond" ? 1.1 : 0.3)}
      linkOpacity={0.45}
      onNodeClick={(n) => {
        onSelect(n);
        if (fgRef.current) flyTo(fgRef.current, n, 80);
      }}
    />
  );
}

function flyTo(fg: ForceGraphMethods<FGNode>, n: FGNode, dist: number) {
  if (n.x == null) return;
  const ratio = 1 + dist / Math.hypot(n.x || 1, n.y || 1, n.z || 1);
  fg.cameraPosition(
    { x: (n.x || 0) * ratio, y: (n.y || 0) * ratio, z: (n.z || 0) * ratio },
    { x: n.x, y: n.y ?? 0, z: n.z ?? 0 },
    900,
  );
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}
