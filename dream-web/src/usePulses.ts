import { useEffect, useRef } from "react";
import type { PulseFeed } from "./api.ts";

const POLL_MS = 2000;

/** Polls the read-only live recall feed and records, per memory label, the
 * wall-clock time it last fired. The returned Map is stable (same object across
 * renders) and mutated in place, so the 3D layer can read it every frame
 * without re-rendering React. */
export function usePulses(enabled: boolean): {
  pulses: Map<string, number>;
  lastFired: React.MutableRefObject<number>;
} {
  const pulses = useRef<Map<string, number>>(new Map()).current;
  const cursor = useRef<string | null>(null);
  const lastFired = useRef<number>(0);

  useEffect(() => {
    if (!enabled) return;
    let stop = false;
    let timer: ReturnType<typeof setTimeout>;

    const tick = async () => {
      try {
        const url = cursor.current
          ? `/api/pulses?since=${encodeURIComponent(cursor.current)}`
          : `/api/pulses`;
        const res = await fetch(url);
        if (res.ok) {
          const feed = (await res.json()) as PulseFeed;
          cursor.current = feed.now;
          if (feed.pulses.length > 0) {
            const now = Date.now();
            for (const p of feed.pulses) pulses.set(p.label, now);
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
