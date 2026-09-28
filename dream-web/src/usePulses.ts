import { useEffect, useRef, useState } from "react";
import type { ActivityAction, ActivityFeed, ActivityPulse } from "./api.ts";

const POLL_MS = 2000;
const STREAM_CAP = 20; // most recent firings kept for the cognition ticker
const CLUSTER_MS = 1500; // recall/surface pulses within this gap = one recall
const EVENT_GAP_MS = 1600; // spacing between successive recall events in a poll — one, then another a beat later

/** One live firing: when it happened (wall-clock ms) and which MCP action lit
 * the node — the 3D layer maps `action` to a colour/animation. */
export interface Firing {
  t: number;
  action: ActivityAction;
}

/** A ticker entry: a firing with the node label + a stable id for React keys.
 * `group` carries the recall's whole co-activation set (the direct hits plus
 * the associatively surfaced neighbours from the same poll) when this firing
 * belongs to one, so hovering the row can show that recall's area. */
export interface StreamEntry {
  id: number;
  label: string;
  action: ActivityAction;
  t: number;
  group?: string[];
}

/** Polls the read-only brain-activity feed and records, per memory label, its
 * most recent firing. `pulses` is a stable Map, mutated in place so the 3D
 * layer can read it every frame without re-rendering React. `stream` is a
 * capped, newest-first list driving the cognition ticker (React state, updated
 * once per poll); recall/surface entries carry their co-activation `group`. */
export function usePulses(enabled: boolean): {
  pulses: Map<string, Firing>;
  stream: StreamEntry[];
  lastFired: React.MutableRefObject<number>;
} {
  const pulses = useRef<Map<string, Firing>>(new Map()).current;
  const cursor = useRef<string | null>(null);
  const lastFired = useRef<number>(0);
  const nextId = useRef<number>(0);
  const pendingTimers = useRef<Set<ReturnType<typeof setTimeout>>>(new Set()).current;
  const [stream, setStream] = useState<StreamEntry[]>([]);

  useEffect(() => {
    if (!enabled) return;
    let stop = false;
    let timer: ReturnType<typeof setTimeout>;

    const tick = async () => {
      try {
        const url = cursor.current
          ? `/api/activity?since=${encodeURIComponent(cursor.current)}`
          : `/api/activity`;
        const res = await fetch(url);
        if (res.ok) {
          const feed = (await res.json()) as ActivityFeed;
          const firstPoll = cursor.current === null;
          cursor.current = feed.now;
          if (feed.pulses.length > 0) {
            if (firstPoll) {
              // The very first poll is history, not live activity: light the
              // backlog subtly all at once and keep it out of the ticker.
              const now = Date.now();
              for (const p of feed.pulses) pulses.set(p.label, { t: now, action: p.action });
              lastFired.current = now;
            } else {
              // One recall writes its direct hit + associatively surfaced
              // neighbours to the event log in a sub-second burst; distinct
              // recalls are seconds/minutes apart. So sort the recall/surface
              // pulses by time and split into clusters wherever the gap exceeds
              // CLUSTER_MS — each cluster is one recall's whole co-activation
              // set (the white hit AND its purple echoes). A cluster with ≥2
              // members becomes the group every one of its rows carries, so
              // hovering any row shows that recall's own connected area, and
              // different recalls get different shapes.
              const rs = feed.pulses
                .filter((p) => p.action === "recall" || p.action === "surface")
                .map((p) => ({ label: p.label, ms: Date.parse(p.at) }))
                .sort((a, b) => a.ms - b.ms);
              const clusters: string[][] = [];
              let cur: string[] = [];
              let lastMs = -Infinity;
              for (const { label, ms } of rs) {
                if (cur.length && ms - lastMs > CLUSTER_MS) {
                  clusters.push(cur);
                  cur = [];
                }
                if (!cur.includes(label)) cur.push(label);
                lastMs = ms;
              }
              if (cur.length) clusters.push(cur);
              const groupByLabel = new Map<string, string[]>();
              for (const c of clusters) {
                if (c.length < 2) continue;
                for (const l of c) groupByLabel.set(l, c);
              }
              // A recall event is a whole cluster — the direct hit AND its
              // surfaced echoes — and it should BLOOM as one (that IS the
              // nebula). What must be paced is one recall event vs the NEXT:
              // group the burst into events (each recall/surface cluster is one
              // event; every other action is its own), order them by time, and
              // release them EVENT_GAP_MS apart. So a recall lights all at once,
              // then — if more arrived this poll — the next follows a beat
              // later, never everything firing bam-bam-bam.
              const clusterIndexOf = new Map<string, number>();
              clusters.forEach((c, i) => c.forEach((l) => clusterIndexOf.set(l, i)));
              type Ev = { at: number; pulses: ActivityPulse[] };
              const events: Ev[] = [];
              const clusterEvent = new Map<number, Ev>();
              for (const p of feed.pulses) {
                const ms = Date.parse(p.at);
                const ci =
                  p.action === "recall" || p.action === "surface"
                    ? clusterIndexOf.get(p.label)
                    : undefined;
                if (ci !== undefined) {
                  let ev = clusterEvent.get(ci);
                  if (!ev) {
                    ev = { at: ms, pulses: [] };
                    clusterEvent.set(ci, ev);
                    events.push(ev);
                  }
                  ev.pulses.push(p);
                  ev.at = Math.min(ev.at, ms);
                } else {
                  events.push({ at: ms, pulses: [p] });
                }
              }
              events.sort((a, b) => a.at - b.at);
              // within an event the recall hit leads (top of the ticker)
              const rank = (a: ActivityAction) => (a === "recall" ? 0 : a === "surface" ? 1 : 2);
              for (const ev of events) ev.pulses.sort((a, b) => rank(a.action) - rank(b.action));

              events.forEach((ev, i) => {
                let handle: ReturnType<typeof setTimeout> | undefined;
                const fire = () => {
                  if (handle !== undefined) pendingTimers.delete(handle);
                  if (stop) return;
                  const at = Date.now();
                  const entries: StreamEntry[] = ev.pulses.map((p) => {
                    pulses.set(p.label, { t: at, action: p.action });
                    const g =
                      p.action === "recall" || p.action === "surface"
                        ? groupByLabel.get(p.label)
                        : undefined;
                    return {
                      id: nextId.current++,
                      label: p.label,
                      action: p.action,
                      t: at,
                      group: g,
                    };
                  });
                  lastFired.current = at;
                  setStream((s) => [...entries, ...s].slice(0, STREAM_CAP));
                };
                if (i === 0) {
                  fire();
                } else {
                  handle = setTimeout(fire, i * EVENT_GAP_MS);
                  pendingTimers.add(handle);
                }
              });
            }
          }
        }
      } catch {
        // transient: keep polling
      }
      if (!stop) timer = setTimeout(tick, POLL_MS);
    };

    tick();
    return () => {
      stop = true;
      clearTimeout(timer);
      for (const h of pendingTimers) clearTimeout(h);
      pendingTimers.clear();
    };
  }, [enabled, pulses, pendingTimers]);

  return { pulses, stream, lastFired };
}
