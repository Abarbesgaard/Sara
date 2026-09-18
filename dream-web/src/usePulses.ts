import { useEffect, useRef, useState } from "react";
import type { ActivityAction, ActivityFeed } from "./api.ts";

const POLL_MS = 2000;
const STREAM_CAP = 20; // most recent firings kept for the cognition ticker

/** One live firing: when it happened (wall-clock ms) and which MCP action lit
 * the node — the 3D layer maps `action` to a colour/animation. */
export interface Firing {
  t: number;
  action: ActivityAction;
}

/** A ticker entry: a firing with the node label + a stable id for React keys. */
export interface StreamEntry {
  id: number;
  label: string;
  action: ActivityAction;
  t: number;
}

/** Polls the read-only brain-activity feed and records, per memory label, its
 * most recent firing. `pulses` is a stable Map, mutated in place so the 3D
 * layer can read it every frame without re-rendering React. `stream` is a
 * capped, newest-first list driving the cognition ticker (React state, updated
 * once per poll). */
export function usePulses(enabled: boolean): {
  pulses: Map<string, Firing>;
  stream: StreamEntry[];
  lastFired: React.MutableRefObject<number>;
} {
  const pulses = useRef<Map<string, Firing>>(new Map()).current;
  const cursor = useRef<string | null>(null);
  const lastFired = useRef<number>(0);
  const nextId = useRef<number>(0);
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
            const now = Date.now();
            for (const p of feed.pulses) pulses.set(p.label, { t: now, action: p.action });
            lastFired.current = now;
            // Skip the very first poll's backlog in the ticker (it's history,
            // not live activity) — but still let it pulse the graph subtly.
            if (!firstPoll) {
              const fresh: StreamEntry[] = feed.pulses.map((p) => ({
                id: nextId.current++,
                label: p.label,
                action: p.action,
                t: now,
              }));
              setStream((s) => [...fresh.reverse(), ...s].slice(0, STREAM_CAP));
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
    };
  }, [enabled, pulses]);

  return { pulses, stream, lastFired };
}
