import { useEffect, useRef } from "react";
import type { ActivityAction, ActivityFeed } from "./api.ts";

const POLL_MS = 2000;

/** One live firing: when it happened (wall-clock ms) and which MCP action lit
 * the node — the 3D layer maps `action` to a colour/animation. */
export interface Firing {
  t: number;
  action: ActivityAction;
}

/** Polls the read-only brain-activity feed and records, per memory label, its
 * most recent firing. The returned Map is stable (same object across renders)
 * and mutated in place, so the 3D layer can read it every frame without
 * re-rendering React. */
export function usePulses(enabled: boolean): {
  pulses: Map<string, Firing>;
  lastFired: React.MutableRefObject<number>;
} {
  const pulses = useRef<Map<string, Firing>>(new Map()).current;
  const cursor = useRef<string | null>(null);
  const lastFired = useRef<number>(0);

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
          cursor.current = feed.now;
          if (feed.pulses.length > 0) {
            const now = Date.now();
            for (const p of feed.pulses) pulses.set(p.label, { t: now, action: p.action });
            lastFired.current = now;
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

  return { pulses, lastFired };
}
