import { useEffect, useMemo, useRef, useState } from "react";
import ForceGraph3D, { type ForceGraphMethods } from "react-force-graph-3d";
import SpriteText from "three-spritetext";
import * as THREE from "three";
import type { Graph, GraphNode } from "../api.ts";
import type { ActivityAction } from "../api.ts";
import type { Firing } from "../usePulses.ts";
import { nodeColor } from "../color.ts";
import type { ViewSettings } from "./ViewToggles.tsx";

interface Props {
  graph: Graph;
  matches: (n: GraphNode) => boolean; // true when the node passes the active filters
  filterSig: string; // changes whenever the active filter set changes
  filtersActive: boolean;
  onSelect: (n: GraphNode) => void;
  focusLabel: string | null; // fly-to target from the search box
  pulses: Map<string, Firing>; // label -> latest firing (when + which MCP action)
  view: ViewSettings; // visual-effect toggles (signals / biolum)
}

// react-force-graph mutates node objects with x/y/z; keep our fields alongside.
type FGNode = GraphNode & { x?: number; y?: number; z?: number };

const DIM = "#23262b";
const PULSE_MS = 1900; // how long a recalled node keeps glowing
const RIPPLE_MS = 1400; // how long a shockwave ring lives

// The colour a node flashes toward for each MCP action — the neurotransmitter
// palette. A node lights up to this hue on fire, then eases back to its normal
// project colour. Ring + signal particles borrow the same hue.
const ACTION_COLOR: Record<ActivityAction, THREE.Color> = {
  recall: new THREE.Color(1.0, 1.0, 1.0), // white — sensory recall
  surface: new THREE.Color(0.8, 0.86, 1.0), // soft blue-white — surfaced
  learn: new THREE.Color(0.35, 1.0, 0.55), // green — memory encoded
  link: new THREE.Color(0.35, 0.85, 1.0), // cyan — synapse formed
  task: new THREE.Color(1.0, 0.72, 0.3), // amber — intention / motor
};
const ACTION_HEX: Record<ActivityAction, number> = {
  recall: 0xffffff,
  surface: 0xcfe0ff,
  learn: 0x5cff8d,
  link: 0x5cd8ff,
  task: 0xffb84d,
};

function desaturate(hsl: string): string {
  // lower the saturation/lightness of an hsl() colour for provisional nodes
  return hsl.replace(/hsl\((\d+),\s*\d+%,\s*\d+%\)/, "hsl($1, 30%, 38%)");
}

// A soft radial-gradient sprite texture, built once, reused for every shockwave
// ring — an expanding light ripple around a node the moment it fires. Baked in
// neutral white so each ring can be tinted to its action colour at spawn.
let RIPPLE_TEX: THREE.Texture | null = null;
function rippleTexture(): THREE.Texture {
  if (RIPPLE_TEX) return RIPPLE_TEX;
  const s = 128;
  const c = document.createElement("canvas");
  c.width = c.height = s;
  const ctx = c.getContext("2d")!;
  const g = ctx.createRadialGradient(s / 2, s / 2, s * 0.30, s / 2, s / 2, s * 0.5);
  g.addColorStop(0, "rgba(255,255,255,0)");
  g.addColorStop(0.72, "rgba(255,255,255,0.55)");
  g.addColorStop(0.92, "rgba(255,255,255,0.95)");
  g.addColorStop(1, "rgba(255,255,255,0)");
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, s, s);
  RIPPLE_TEX = new THREE.CanvasTexture(c);
  return RIPPLE_TEX;
}

export function Graph3D({
  graph,
  matches,
  filterSig,
  filtersActive,
  onSelect,
  focusLabel,
  pulses,
  view,
}: Props) {
  const fgRef = useRef<ForceGraphMethods<FGNode> | undefined>(undefined);
  const fitDoneRef = useRef(false);
  const gridRef = useRef<THREE.GridHelper | null>(null);
  const shellRef = useRef<THREE.Mesh | null>(null);

  // Latest toggle set, readable from the imperative animation loop without
  // re-subscribing it every time a switch flips.
  const viewRef = useRef(view);
  viewRef.current = view;

  // Colour the next emitted signal particles should use — set just before an
  // emit so the graph-level particle accessor can tint per firing action.
  const signalColorRef = useRef<number>(0xffe08a);

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

  // Filter / signal-shape / palette changes only affect colour, size & link
  // curvature — redraw without reheating the layout.
  useEffect(() => {
    fgRef.current?.refresh();
  }, [filterSig, view.signals, view.biolum]);

  const baseVal = (n: FGNode) => Math.max(1, (n.strength - 0.9) * 6);

  const colorFor = (n: FGNode): string => {
    if (filtersActive && !matches(n)) return DIM;
    const base = nodeColor(n.projects, view.biolum);
    return n.provisional ? desaturate(base) : base;
  };

  // Fast lookup from uuid -> live node object (for resolving link endpoints).
  const nodeByUuid = useMemo(() => {
    const m = new Map<string, FGNode>();
    for (const n of data.nodes) m.set(n.id, n);
    return m;
  }, [data]);

  const endpointLabel = (x: unknown): string | undefined => {
    if (x && typeof x === "object") return (x as FGNode).label;
    if (typeof x === "string") return nodeByUuid.get(x)?.label;
    return undefined;
  };

  // Fly the camera to a searched node.
  useEffect(() => {
    if (!focusLabel || !fgRef.current) return;
    const n = data.nodes.find((x) => x.label === focusLabel);
    if (n == null || n.x == null) return;
    flyTo(fgRef.current, n, 100);
  }, [focusLabel, data]);

  // --- Activation loop: on recall a node flares bright, swells, and throws off
  // an expanding shockwave ring; with `signals` on, particles also fire along
  // its bonds. One rAF loop mutates the meshes every frame and reads the live
  // toggle set via viewRef, so flipping a switch never restarts it. Nodes are
  // otherwise still (no idle breathing).
  useEffect(() => {
    const ripples: { sprite: THREE.Sprite; start: number; n: FGNode }[] = [];
    const lastEmit = new Map<string, number>();
    const flare = new THREE.Color();
    const black = new THREE.Color(0, 0, 0);
    const baseCol = new THREE.Color();
    const outCol = new THREE.Color();
    // Nodes mid-flash: remember the action colour so cooling frames ease back
    // from the right hue, and so we restore the base colour exactly once cool.
    const lit = new Map<string, ActivityAction>();
    let raf = 0;

    const spawnRipple = (n: FGNode, action: ActivityAction) => {
      const scene = fgRef.current?.scene?.();
      if (!scene || n.x == null) return;
      const mat = new THREE.SpriteMaterial({
        map: rippleTexture(),
        color: ACTION_HEX[action],
        transparent: true,
        blending: THREE.AdditiveBlending,
        depthWrite: false,
        opacity: 0.95,
      });
      const sprite = new THREE.Sprite(mat);
      sprite.position.set(n.x, n.y ?? 0, n.z ?? 0);
      scene.add(sprite);
      ripples.push({ sprite, start: Date.now(), n });
    };

    const emitSignals = (label: string, action: ActivityAction) => {
      const fg = fgRef.current;
      if (!fg) return;
      signalColorRef.current = ACTION_HEX[action];
      for (const l of data.links as unknown as { source: unknown; target: unknown }[]) {
        if (endpointLabel(l.source) === label || endpointLabel(l.target) === label) {
          fg.emitParticle(l as never);
        }
      }
    };

    const loop = () => {
      const v = viewRef.current;
      const now = Date.now();

      for (const n of data.nodes) {
        const obj = (n as unknown as { __threeObj?: THREE.Object3D }).__threeObj;
        if (!obj) continue;
        const fired = pulses.get(n.label);
        const firedAt = fired?.t;
        const action: ActivityAction = fired?.action ?? lit.get(n.label) ?? "recall";
        const intensity = firedAt ? Math.max(0, 1 - (now - firedAt) / PULSE_MS) : 0;

        // New fire this frame → shockwave ring (+ signals if enabled), once.
        if (firedAt && lastEmit.get(n.label) !== firedAt) {
          lastEmit.set(n.label, firedAt);
          spawnRipple(n, action);
          if (v.signals) emitSignals(n.label, action);
        }

        // Swell while active. Light the node up: its material colour snaps to
        // the action's hue on fire, then eases back to its normal project
        // colour as it cools (a lighter emissive adds a touch of extra glow).
        obj.scale.setScalar(1 + 2.3 * intensity);

        const isLit = lit.has(n.label);
        if (intensity > 0 || isLit) {
          baseCol.set(colorFor(n));
          outCol.copy(baseCol).lerp(ACTION_COLOR[action], intensity); // 1 => action hue, 0 => base
          flare.setRGB(0.7 * intensity, 0.7 * intensity, 0.7 * intensity);
          obj.traverse((c) => {
            const mat = (c as THREE.Mesh).material as THREE.MeshLambertMaterial | undefined;
            if (!mat) return;
            if ((mat as unknown as { color?: THREE.Color }).color) {
              (mat as unknown as { color: THREE.Color }).color.copy(outCol);
            }
            if ((mat as unknown as { emissive?: THREE.Color }).emissive) {
              (mat as unknown as { emissive: THREE.Color }).emissive.copy(
                intensity > 0 ? flare : black,
              );
            }
          });
          if (intensity > 0) lit.set(n.label, action);
          else lit.delete(n.label); // cooled: leave it resting at base colour
        }

        if (firedAt && intensity <= 0) pulses.delete(n.label);
      }

      // Advance + retire shockwave rings.
      for (let i = ripples.length - 1; i >= 0; i--) {
        const r = ripples[i];
        const age = (now - r.start) / RIPPLE_MS;
        if (age >= 1) {
          fgRef.current?.scene?.()?.remove(r.sprite);
          (r.sprite.material as THREE.SpriteMaterial).dispose();
          ripples.splice(i, 1);
          continue;
        }
        const scale = baseVal(r.n) * (2.4 + age * 15);
        r.sprite.scale.setScalar(scale);
        (r.sprite.material as THREE.SpriteMaterial).opacity = 0.95 * (1 - age);
        if (r.n.x != null) r.sprite.position.set(r.n.x, r.n.y ?? 0, r.n.z ?? 0);
      }

      raf = requestAnimationFrame(loop);
    };

    raf = requestAnimationFrame(loop);
    return () => {
      cancelAnimationFrame(raf);
      for (const r of ripples) {
        fgRef.current?.scene?.()?.remove(r.sprite);
        (r.sprite.material as THREE.SpriteMaterial).dispose();
      }
    };
  }, [data, pulses, nodeByUuid]);

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

    if (!gridRef.current) {
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
      linkCurvature={view.signals ? 0.18 : 0}
      linkColor={(l) => ((l as unknown as { kind: string }).kind === "bond" ? "#6ea8fe" : "#333941")}
      linkWidth={(l) => ((l as unknown as { kind: string }).kind === "bond" ? 1.1 : 0.3)}
      linkOpacity={0.45}
      linkDirectionalParticles={(l) =>
        view.signals && (l as unknown as { kind: string }).kind === "bond" ? 2 : 0
      }
      linkDirectionalParticleSpeed={0.006}
      linkDirectionalParticleWidth={1.3}
      linkDirectionalParticleColor={() =>
        `#${signalColorRef.current.toString(16).padStart(6, "0")}`
      }
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
