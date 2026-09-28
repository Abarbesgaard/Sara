import { useEffect, useMemo, useRef, useState } from "react";
import ForceGraph3D, { type ForceGraphMethods } from "react-force-graph-3d";
import SpriteText from "three-spritetext";
import * as THREE from "three";
import { ConvexGeometry } from "three/examples/jsm/geometries/ConvexGeometry.js";
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
  highlightLabel: string | null; // ticker-hover spotlight target
  highlightGroup: string[] | null; // ticker-hover recall group to enclose in a nebula
}

// react-force-graph mutates node objects with x/y/z; keep our fields alongside.
type FGNode = GraphNode & { x?: number; y?: number; z?: number };

const DIM = "#23262b";
const PULSE_MS = 1900; // default glow lifetime; per-action overrides below
const RIPPLE_MS = 1400; // how long a shockwave ring lives

// The colour a node flashes toward for each MCP action — the neurotransmitter
// palette. A node lights up to this hue on fire, then eases back to its normal
// project colour. Ring + signal particles borrow the same hue.
const ACTION_COLOR: Record<ActivityAction, THREE.Color> = {
  recall: new THREE.Color(1.0, 1.0, 1.0), // white — deliberate sensory recall
  surface: new THREE.Color(0.62, 0.36, 1.0), // violet — uninvited associative echo
  learn: new THREE.Color(0.35, 1.0, 0.55), // green — memory encoded
  link: new THREE.Color(0.35, 0.85, 1.0), // cyan — synapse formed
  task: new THREE.Color(1.0, 0.72, 0.3), // amber — intention / motor
};
const ACTION_HEX: Record<ActivityAction, number> = {
  recall: 0xffffff,
  surface: 0x9b6bff,
  learn: 0x5cff8d,
  link: 0x5cd8ff,
  task: 0xffb84d,
};

// How hard each action fires. `surface` is an uninvited associative hit that
// never reinforces strength, so it reads as a softer, quicker shimmer — a
// background echo — while a deliberate `recall` punches bright and lingers.
const ACTION_SWELL: Record<ActivityAction, number> = {
  recall: 2.6,
  surface: 1.0,
  learn: 2.3,
  link: 2.3,
  task: 2.3,
};
const ACTION_PULSE_MS: Record<ActivityAction, number> = {
  recall: 2100,
  surface: 1000,
  learn: 1900,
  link: 1900,
  task: 1900,
};

// Steady spotlight when a ticker row is hovered — a warm gold that reads apart
// from every action hue, so pointing at a row instantly finds its node.
const HIGHLIGHT_COLOR = new THREE.Color(1.0, 0.86, 0.35);
const HIGHLIGHT_SWELL = 2.4;

// Recall "nebula": while a recall row is hovered, ONE smooth translucent hull
// volume encloses that recall's whole activation set — the recalled "main"
// memory PLUS every memory it activated (the surfaced echoes) — as a single
// generous cloud/territory rather than a bubble per node. It's the convex hull
// of the members, each inflated by a padding sphere so the surface is rounded
// and roomy. The main memory also gets a brighter glowing core so you can see
// which memory the recall centred on. Additive blending keeps it see-through.
const CLOUD_HEX = 0x8fa6ff; // soft indigo nebula gas
const CLOUD_CORE_HEX = 0xdfe6ff; // brighter near-white core on the main memory
const CLOUD_LINE_HEX = 0xbcd0ff; // constellation lines linking the members
const CLOUD_PAD = 46; // padding radius around each node — the halo's generosity
const CLOUD_OPACITY = 0.07; // additive; very faint so the hull is barely-there gas
const CLOUD_LINE_OPACITY = 0.5; // constellation line brightness
const CLOUD_CORE_R = 46; // radius of the bright core glow sprite on the main node
const CLOUD_CORE_OPACITY = 0.5; // peak opacity of the core glow
const CLOUD_FADE_MS = 320; // ease-in/out when the hover enters/leaves

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

// A soft filled radial glow (bright centre → transparent edge), built once and
// tinted per use — the bright core marking the recall's main memory.
let GLOW_TEX: THREE.Texture | null = null;
function glowTexture(): THREE.Texture {
  if (GLOW_TEX) return GLOW_TEX;
  const s = 128;
  const c = document.createElement("canvas");
  c.width = c.height = s;
  const ctx = c.getContext("2d")!;
  const g = ctx.createRadialGradient(s / 2, s / 2, 0, s / 2, s / 2, s / 2);
  g.addColorStop(0, "rgba(255,255,255,1)");
  g.addColorStop(0.35, "rgba(255,255,255,0.5)");
  g.addColorStop(0.7, "rgba(255,255,255,0.12)");
  g.addColorStop(1, "rgba(255,255,255,0)");
  ctx.fillStyle = g;
  ctx.fillRect(0, 0, s, s);
  GLOW_TEX = new THREE.CanvasTexture(c);
  return GLOW_TEX;
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
  highlightLabel,
  highlightGroup,
}: Props) {
  const fgRef = useRef<ForceGraphMethods<FGNode> | undefined>(undefined);
  const fitDoneRef = useRef(false);
  const gridRef = useRef<THREE.GridHelper | null>(null);
  const shellRef = useRef<THREE.Mesh | null>(null);

  // Latest toggle set, readable from the imperative animation loop without
  // re-subscribing it every time a switch flips.
  const viewRef = useRef(view);
  viewRef.current = view;

  // Latest ticker-hover target, read from the animation loop the same way.
  const highlightRef = useRef(highlightLabel);
  highlightRef.current = highlightLabel;

  // Latest hovered recall group, read from the animation loop without restarting it.
  const highlightGroupRef = useRef(highlightGroup);
  highlightGroupRef.current = highlightGroup;

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

  // Fast lookup from label -> live node object (for cascade cloud endpoints).
  const nodeByLabel = useMemo(() => {
    const m = new Map<string, FGNode>();
    for (const n of data.nodes) m.set(n.label, n);
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
    // Nodes currently held bright by a ticker hover, so we can restore them the
    // frame the pointer leaves.
    const hoverStyled = new Set<string>();
    let raf = 0;

    // The single hover nebula: one static translucent hull volume enclosing the
    // whole recall group, plus a brighter glow sprite marking the main memory.
    // `sig` identifies the group+main so we rebuild only when the hovered recall
    // changes; `vis` eases opacity in/out on hover enter/leave.
    interface Nebula {
      mesh: THREE.Mesh;
      lines: THREE.LineSegments;
      core: THREE.Sprite;
      sig: string;
      vis: number; // 0..1 eased visibility
    }
    let nebula: Nebula | null = null;

    const nebulaSig = (g: string[] | null, main: string | null): string =>
      g && g.length >= 2 ? (main ?? "") + "»" + [...g].sort().join("|") : "";

    // Directions on a unit sphere (Fibonacci) used to inflate each node into a
    // padding ball of sample points; the convex hull of all of them is a smooth,
    // rounded, generous region hugging the whole group.
    const SPHERE_DIRS: [number, number, number][] = (() => {
      const n = 18;
      const out: [number, number, number][] = [];
      const golden = Math.PI * (3 - Math.sqrt(5));
      for (let i = 0; i < n; i++) {
        const y = 1 - (i / (n - 1)) * 2;
        const r = Math.sqrt(Math.max(0, 1 - y * y));
        const th = golden * i;
        out.push([Math.cos(th) * r, y, Math.sin(th) * r]);
      }
      return out;
    })();

    const disposeNebula = () => {
      if (!nebula) return;
      const scene = fgRef.current?.scene?.();
      scene?.remove(nebula.mesh);
      scene?.remove(nebula.lines);
      scene?.remove(nebula.core);
      nebula.mesh.geometry.dispose();
      (nebula.mesh.material as THREE.Material).dispose();
      nebula.lines.geometry.dispose();
      (nebula.lines.material as THREE.Material).dispose();
      (nebula.core.material as THREE.Material).dispose();
      nebula = null;
    };

    // Build one generous convex-hull cloud over the whole recall group: inflate
    // every member node into a padding sphere of points, then take the convex
    // hull of all those points — a single rounded translucent territory rather
    // than a bubble per node. A bright glow sprite marks the main memory.
    const buildNebula = (labels: string[], mainLabel: string | null): Nebula | null => {
      const scene = fgRef.current?.scene?.();
      const nodes = labels
        .map((l) => nodeByLabel.get(l))
        .filter((n): n is FGNode => !!n && n.x != null);
      if (!scene || nodes.length < 2) return null;

      const mainNode = (mainLabel && nodeByLabel.get(mainLabel)) || nodes[0];
      const mx = mainNode.x!, my = mainNode.y ?? 0, mz = mainNode.z ?? 0;

      const points: THREE.Vector3[] = [];
      for (const n of nodes) {
        const nx = n.x!, ny = n.y ?? 0, nz = n.z ?? 0;
        for (const [dx, dy, dz] of SPHERE_DIRS) {
          points.push(
            new THREE.Vector3(
              nx + dx * CLOUD_PAD,
              ny + dy * CLOUD_PAD,
              nz + dz * CLOUD_PAD,
            ),
          );
        }
      }

      const geo = new ConvexGeometry(points);
      const mat = new THREE.MeshBasicMaterial({
        color: CLOUD_HEX,
        transparent: true,
        opacity: 0,
        side: THREE.DoubleSide,
        depthWrite: false,
        blending: THREE.AdditiveBlending,
      });
      const mesh = new THREE.Mesh(geo, mat);
      mesh.renderOrder = 1;
      scene.add(mesh);

      // Constellation lines: a faint filament from the main memory to each
      // activated peripheral, so the group reads as a linked star pattern.
      const segs: number[] = [];
      for (const n of nodes) {
        if (n === mainNode) continue;
        segs.push(mx, my, mz, n.x!, n.y ?? 0, n.z ?? 0);
      }
      const lineGeo = new THREE.BufferGeometry();
      lineGeo.setAttribute("position", new THREE.Float32BufferAttribute(segs, 3));
      const lines = new THREE.LineSegments(
        lineGeo,
        new THREE.LineBasicMaterial({
          color: CLOUD_LINE_HEX,
          transparent: true,
          opacity: 0,
          depthWrite: false,
          blending: THREE.AdditiveBlending,
        }),
      );
      lines.renderOrder = 2;
      scene.add(lines);

      const core = new THREE.Sprite(
        new THREE.SpriteMaterial({
          map: glowTexture(),
          color: CLOUD_CORE_HEX,
          transparent: true,
          opacity: 0,
          depthWrite: false,
          blending: THREE.AdditiveBlending,
        }),
      );
      core.position.set(mx, my, mz);
      core.scale.setScalar(CLOUD_CORE_R * 2);
      core.renderOrder = 2;
      scene.add(core);

      return { mesh, lines, core, sig: nebulaSig(labels, mainLabel), vis: 0 };
    };

    // Ease the static cloud's opacity toward the hovered/not-hovered target;
    // rebuild only when the hovered recall (group or main memory) changes.
    const updateNebula = (group: string[] | null, mainLabel: string | null) => {
      const sig = nebulaSig(group, mainLabel);
      if (sig !== (nebula?.sig ?? "")) {
        disposeNebula();
        if (sig !== "") nebula = buildNebula(group!, mainLabel);
      }
      const nb = nebula;
      if (!nb) return;

      const target = sig !== "" ? 1 : 0;
      const dt = 16 / CLOUD_FADE_MS;
      nb.vis += Math.sign(target - nb.vis) * Math.min(Math.abs(target - nb.vis), dt);
      if (nb.vis <= 0 && target === 0) {
        disposeNebula();
        return;
      }
      (nb.mesh.material as THREE.MeshBasicMaterial).opacity = CLOUD_OPACITY * nb.vis;
      (nb.lines.material as THREE.LineBasicMaterial).opacity = CLOUD_LINE_OPACITY * nb.vis;
      (nb.core.material as THREE.SpriteMaterial).opacity = CLOUD_CORE_OPACITY * nb.vis;
    };

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
      const hl = highlightRef.current;
      const now = Date.now();

      for (const n of data.nodes) {
        const obj = (n as unknown as { __threeObj?: THREE.Object3D }).__threeObj;
        if (!obj) continue;
        const fired = pulses.get(n.label);
        const firedAt = fired?.t;
        const action: ActivityAction = fired?.action ?? lit.get(n.label) ?? "recall";
        const pulseMs = ACTION_PULSE_MS[action] ?? PULSE_MS;
        const intensity = firedAt ? Math.max(0, 1 - (now - firedAt) / pulseMs) : 0;
        const hovered = hl != null && n.label === hl;
        const wasHovered = hoverStyled.has(n.label);

        // New fire this frame → shockwave ring (+ signals if enabled), once.
        if (firedAt && lastEmit.get(n.label) !== firedAt) {
          lastEmit.set(n.label, firedAt);
          spawnRipple(n, action);
          if (v.signals) emitSignals(n.label, action);
        }

        // Swell while active; a hover holds a steady swell on top of any pulse.
        const swell = 1 + (ACTION_SWELL[action] ?? 2.3) * intensity;
        obj.scale.setScalar(hovered ? Math.max(swell, 1 + HIGHLIGHT_SWELL) : swell);

        const isLit = lit.has(n.label);
        if (intensity > 0 || isLit || hovered || wasHovered) {
          baseCol.set(colorFor(n));
          outCol.copy(baseCol).lerp(ACTION_COLOR[action], intensity); // 1 => action hue, 0 => base
          flare.copy(ACTION_COLOR[action]).multiplyScalar(0.7 * intensity); // glow in the action's hue, not white
          if (hovered) {
            // Steady gold spotlight, layered over (and winning against) any pulse.
            outCol.lerp(HIGHLIGHT_COLOR, 0.7);
            flare.copy(HIGHLIGHT_COLOR).multiplyScalar(0.55);
            hoverStyled.add(n.label);
          } else if (wasHovered) {
            hoverStyled.delete(n.label); // pointer left: fall back to pulse/base below
          }
          const emissive = intensity > 0 || hovered ? flare : black;
          obj.traverse((c) => {
            const mat = (c as THREE.Mesh).material as THREE.MeshLambertMaterial | undefined;
            if (!mat) return;
            if ((mat as unknown as { color?: THREE.Color }).color) {
              (mat as unknown as { color: THREE.Color }).color.copy(outCol);
            }
            if ((mat as unknown as { emissive?: THREE.Color }).emissive) {
              (mat as unknown as { emissive: THREE.Color }).emissive.copy(emissive);
            }
          });
          if (intensity > 0) lit.set(n.label, action);
          else lit.delete(n.label); // cooled: leave it resting at base colour (hover handled above)
        }

        if (firedAt && intensity <= 0) pulses.delete(n.label);
      }

      // Recall nebula: enclose the hovered ticker row's whole recall group.
      updateNebula(highlightGroupRef.current, highlightRef.current);
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
      disposeNebula();
    };
  }, [data, pulses, nodeByUuid, nodeByLabel]);

  // Steady view: settle the layout and make one final camera fit, then leave
  // the camera still (no auto-rotation, so the cloud doesn't drift side to side).
  const onSettled = () => {
    disableRotation();
    syncReference();
    if (!fitDoneRef.current) {
      fgRef.current?.zoomToFit(700, 90);
      fitDoneRef.current = true;
    }
  };

  const disableRotation = () => {
    const c = fgRef.current?.controls() as
      | { autoRotate?: boolean; autoRotateSpeed?: number }
      | undefined;
    if (c) {
      c.autoRotate = false;
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
