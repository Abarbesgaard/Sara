import type { ActivityAction } from "../api.ts";
import type { StreamEntry } from "../usePulses.ts";

// Per-action colour + verb, mirroring the 3D flash palette so the ticker reads
// as a running commentary of what the brain is doing.
const ACTION_META: Record<ActivityAction, { color: string; verb: string }> = {
  recall: { color: "#ffffff", verb: "recall" },
  surface: { color: "#cfe0ff", verb: "surface" },
  learn: { color: "#5cff8d", verb: "learn" },
  link: { color: "#5cd8ff", verb: "link" },
  task: { color: "#ffb84d", verb: "task" },
};

function clock(t: number): string {
  const d = new Date(t);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

interface Props {
  stream: StreamEntry[];
  onPick: (label: string) => void;
}

export function Ticker({ stream, onPick }: Props) {
  return (
    <div className="ticker" aria-label="Live cognition feed">
      <div className="ticker-head">
        <span className="ticker-title">cognition</span>
        <span className="ticker-legend">
          {(Object.keys(ACTION_META) as ActivityAction[]).map((a) => (
            <span key={a} className="tk-key" title={a}>
              <span className="tk-swatch" style={{ background: ACTION_META[a].color }} />
              {ACTION_META[a].verb}
            </span>
          ))}
        </span>
      </div>
      <div className="ticker-rows">
        {stream.length === 0 && <div className="ticker-idle">waiting for activity…</div>}
        {stream.map((e) => {
          const meta = ACTION_META[e.action];
          return (
            <button
              key={e.id}
              className="ticker-row"
              title={`${e.action} · ${e.label}`}
              onClick={() => onPick(e.label)}
            >
              <span className="tk-time">{clock(e.t)}</span>
              <span className="tk-verb" style={{ color: meta.color }}>
                {meta.verb}
              </span>
              <span className="tk-node" style={{ color: meta.color }}>
                {e.label}
              </span>
            </button>
          );
        })}
      </div>
    </div>
  );
}
