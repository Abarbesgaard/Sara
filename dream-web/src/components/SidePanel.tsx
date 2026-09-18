import { useEffect, useState } from "react";
import { fetchMemory, type GraphNode, type MemoryDetail } from "../api.ts";
import { projectColor } from "../color.ts";

interface Props {
  node: GraphNode | null;
  onClose: () => void;
}

export function SidePanel({ node, onClose }: Props) {
  const [detail, setDetail] = useState<MemoryDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    if (!node) {
      setDetail(null);
      setError(null);
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    setDetail(null);
    fetchMemory(node.label)
      .then((d) => {
        if (!cancelled) setDetail(d);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [node]);

  if (!node) return null;

  return (
    <aside className="side">
      <div className="side-head">
        <div>
          <span className="side-label">{node.label}</span>
          {node.canonical && <span className="badge">canonical · {node.derivedCount}</span>}
          {node.provisional && <span className="badge prov">provisional</span>}
        </div>
        <button className="close" onClick={onClose}>
          ×
        </button>
      </div>

      <h2 className="side-title">{detail?.title ?? node.title}</h2>

      <div className="side-meta">
        {(detail?.projects ?? node.projects).map((p) => (
          <span key={p} className="chip small">
            <span className="dot" style={{ background: projectColor(p) }} />
            {p}
          </span>
        ))}
        {(detail?.tags ?? node.tags).map((t) => (
          <span key={t} className="chip small">
            {t}
          </span>
        ))}
      </div>

      {loading && <p className="muted">loading…</p>}
      {error && <p className="err">{error}</p>}

      {detail && (
        <>
          <pre className="body">{detail.body || "(no body)"}</pre>

          {detail.tasks.length > 0 && (
            <section>
              <h3>Tasks</h3>
              <ul>
                {detail.tasks.map((t, i) => (
                  <li key={i}>
                    {t.id != null && <b>#{t.id} </b>}
                    {t.description} <span className="muted">({t.source})</span>
                  </li>
                ))}
              </ul>
            </section>
          )}

          {detail.files.length > 0 && (
            <section>
              <h3>Files</h3>
              <ul className="files">
                {detail.files.map((f) => (
                  <li key={f}>{f}</li>
                ))}
              </ul>
            </section>
          )}

          <p className="muted tiny">
            created {fmt(detail.createdAt)} · modified {fmt(detail.modifiedAt)}
          </p>
        </>
      )}
    </aside>
  );
}

function fmt(s: string): string {
  if (!s) return "—";
  const d = new Date(s);
  return Number.isNaN(d.getTime()) ? s : d.toLocaleDateString();
}
