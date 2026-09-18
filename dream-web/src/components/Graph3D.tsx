import { useEffect, useMemo, useRef, useState } from "react";
import ForceGraph3D, { type ForceGraphMethods } from "react-force-graph-3d";
import SpriteText from "three-spritetext";
import * as THREE from "three";
import type { Graph, GraphNode } from "../api.ts";
import { nodeColor } from "../color.ts";

interface Props {
  graph: Graph;
  matches: (n: GraphNode) => boolean; // true when the node passes the active filters
  filterSig: string; // changes whenever the active filter set changes
  filtersActive: boolean;
  onSelect: (n: GraphNode) => void;
  focusLabel: string | null; // fly-to target from the search box
  pulses: Map<string, number>; // label -> wall-clock ms it last fired (live recall)
}

// react-force-graph mutates node objects with x/y/z; keep our fields alongside.
type FGNode = GraphNode & { x?: number; y?: number; z?: number };

const DIM = "#23262b";
const PULSE_MS = 1900; // how long a recalled node keeps glowing

function desaturate(hsl: string): string {
  // lower the saturation/lightness of an hsl() colour for provisional nodes
  return hsl.replace(/hsl\((\d+),\s*\d+%,\s*\d+%\)/, "hsl($1, 30%, 38%)");
}

export function Graph3D({
  graph,
  matches,
  filterSig,
  filtersActive,
  onSelect,
  focusLabel,
  pulses,
}: Props) {
  const fgRef = useRef<ForceGraphMethods<FGNode> | undefined>(undefined);
  const refAddedRef = useRef(false);
  const fitDoneRef = useRef(false);
  const gridRef = useRef<THREE.GridHelper | null>(null);
  const shellRef = useRef<THREE.Mesh | null>(null);

  // react-force-graph falls back to window.innerWidth/Height when no explicit
  // size is given, which overflows the sidebar and pushes the graph's centre
  // off-screen. Measure our own wrapper and drive the canvas size from it.
  const wrapRef = useRef<HTMLDivElement>(null);
  const [size, setSize] = useState({ w: 0, h: 0 });
  useEffect(() => {
    const el = wrapRef.current;
    if (!el) return;
    const measure = () =>
      setSize({ w: el.clientWidth, h: el.clientHeight });
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Keep a single stable graph-data object. Changing its identity would make
  // react-force-graph restart the physics simulation (nodes fly apart), so we
  // build it once per graph and restyle in place on filter changes instead.
  const data = useMemo(
    () => ({
      nodes: graph.nodes as FGNode[],
      links: graph.edges.map((e) => ({ ...e })),
    }),
    [graph],
  );

  // Filter/pulse changes only affect colour & size — redraw without reheating
  // the layout.
  useEffect(() => {
    fgRef.current?.refresh();
  }, [filterSig]);

  const baseVal = (n: FGNode) => Math.max(1, (n.strength - 0.9) * 6);

  const colorFor = (n: FGNode): string => {
    if (filtersActive && !matches(n)) return DIM;
    const base = nodeColor(n.projects);
    return n.provisional ? desaturate(base) : base;
  };

  // Fast lookup from label -> live node object (carries the rendered __threeObj).
  const nodeByLabel = useMemo(() => {
    const m = new Map<string, FGNode>();
    for (const n of data.nodes) m.set(n.label, n);
    return m;
  }, [data]);

  // Fly the camera to a searched node.
  useEffect(() => {
    if (!focusLabel || !fgRef.current) return;
    const n = data.nodes.find((x) => x.label === focusLabel);
    if (n == null || n.x == null) return;
    flyTo(fgRef.current, n, 100);
  }, [focusLabel, data]);

  // Live recall pulse. Rather than asking react-force-graph to re-run its style
  // accessors (unreliable for the 3D renderer), we mutate the node meshes
  // directly on a steady setInterval: a recalled node's sphere swells and glows
  // gold (emissive), then eases back to rest. setInterval — unlike rAF — keeps
  // firing regardless of the render loop, and the graph's own animation loop
  // paints the mutated meshes every frame.
  useEffect(() => {
    const applyPulse = (obj: THREE.Object3D, intensity: number) => {
      obj.scale.setScalar(1 + 1.9 * intensity);
      obj.traverse((c) => {
        const mat = (c as THREE.Mesh).material as
          | THREE.MeshLambertMaterial
          | undefined;
        if (mat && (mat as unknown as { emissive?: THREE.Color }).emissive) {
          (mat as unknown as { emissive: THREE.Color }).emissive.setRGB(
            intensity,
            0.82 * intensity,
            0.32 * intensity,
          );
        }
      });
    };

    const id = window.setInterval(() => {
      if (pulses.size === 0) return;
      const now = Date.now();
      for (const [label, t] of pulses) {
        const n = nodeByLabel.get(label);
        const obj = n && (n as unknown as { __threeObj?: THREE.Object3D }).__threeObj;
        const intensity = Math.max(0, 1 - (now - t) / PULSE_MS);
        if (!obj) {
          if (intensity <= 0) pulses.delete(label);
          continue;
        }
        if (intensity > 0) {
          applyPulse(obj, intensity);
        } else {
          applyPulse(obj, 0); // reset scale + emissive to rest
          pulses.delete(label);
        }
      }
    }, 60);
    return () => window.clearInterval(id);
  }, [pulses, nodeByLabel]);

  // Enable auto-rotation once the layout settles, and make one final camera fit.
  const onSettled = () => {
    enableRotation();
    syncReference();
    if (!fitDoneRef.current) {
      fgRef.current?.zoomToFit(700, 90);
      fitDoneRef.current = true;
    }
  };

  const enableRotation = () => {
    const c = fgRef.current?.controls() as
      | { autoRotate?: boolean; autoRotateSpeed?: number }
      | undefined;
    if (c) {
      c.autoRotate = true;
      c.autoRotateSpeed = 0.22; // subtle
    }
  };

  // Build the spatial reference (ground grid + faint boundary sphere) once, then
  // keep it tracking the cloud on every engine tick so it is visible from the
  // very first frame — not only after the layout settles.
  const syncReference = () => {
    if (!fgRef.current) return;
    const scene = fgRef.current.scene();
    if (!scene) return;

    let minX = Infinity, minY = Infinity, minZ = Infinity;
    let maxX = -Infinity, maxY = -Infinity, maxZ = -Infinity;
    for (const n of data.nodes) {
      const x = n.x ?? 0, y = n.y ?? 0, z = n.z ?? 0;
      minX = Math.min(minX, x); maxX = Math.max(maxX, x);
      minY = Math.min(minY, y); maxY = Math.max(maxY, y);
      minZ = Math.min(minZ, z); maxZ = Math.max(maxZ, z);
    }
    if (!Number.isFinite(minX)) return;
    const cx = (minX + maxX) / 2, cy = (minY + maxY) / 2, cz = (minZ + maxZ) / 2;
    const r = 0.5 * Math.hypot(maxX - minX, maxY - minY, maxZ - minZ) || 120;
    const shellR = r * 0.9; // snug boundary sphere

    if (!refAddedRef.current) {
      const ref = new THREE.Group();
      ref.name = "dream-reference";

      // Unit-sized primitives so they can be re-scaled cheaply as the cloud grows.
      const grid = new THREE.GridHelper(1, 24, 0x4a5568, 0x2a313c);
      const gMat = grid.material as THREE.Material | THREE.Material[];
      (Array.isArray(gMat) ? gMat : [gMat]).forEach((m) => {
        m.transparent = true;
        m.opacity = 0.5;
        m.depthWrite = false;
      });
      ref.add(grid);
      gridRef.current = grid;

      const shell = new THREE.Mesh(
        new THREE.SphereGeometry(1, 24, 16),
        new THREE.MeshBasicMaterial({
          color: 0x38424f,
          wireframe: true,
          transparent: true,
          opacity: 0.14,
          depthWrite: false,
        }),
      );
      ref.add(shell);
      shellRef.current = shell;

      scene.add(ref);
      refAddedRef.current = true;
    }

    const grid = gridRef.current;
    const shell = shellRef.current;
    if (grid) {
      grid.scale.setScalar(shellR * 2);
      grid.position.set(cx, cy - shellR, cz); // bottom of the globe
    }
    if (shell) {
      shell.scale.setScalar(shellR);
      shell.position.set(cx, cy, cz);
    }
  };

  return (
    <div ref={wrapRef} style={{ width: "100%", height: "100%" }}>
    <ForceGraph3D<FGNode>
      ref={fgRef}
      width={size.w || undefined}
      height={size.h || undefined}
      graphData={data}
      backgroundColor="#0b0d10"
      showNavInfo={false}
      controlType="orbit"
      onEngineTick={syncReference}
      onEngineStop={onSettled}
      nodeVal={(n) => baseVal(n)}
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
    </div>
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
