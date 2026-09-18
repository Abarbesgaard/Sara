import { useEffect, useMemo, useState } from "react";
import { fetchGraph, type Graph, type GraphNode } from "./api.ts";
import { Graph3D } from "./components/Graph3D.tsx";
import { SidePanel } from "./components/SidePanel.tsx";
import { ViewToggles, DEFAULT_VIEW, type ViewSettings } from "./components/ViewToggles.tsx";
import { Ticker } from "./components/Ticker.tsx";
import { usePulses } from "./usePulses.ts";

export function App() {
  const [graph, setGraph] = useState<Graph | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [selected, setSelected] = useState<GraphNode | null>(null);
  const [search, setSearch] = useState("");
  const [focusLabel, setFocusLabel] = useState<string | null>(null);
  const [view, setView] = useState<ViewSettings>(DEFAULT_VIEW);
  const toggleView = (key: keyof ViewSettings) =>
    setView((v) => ({ ...v, [key]: !v[key] }));

  // Live recall feed — pulse nodes as other processes recall them.
  const { pulses, stream, lastFired } = usePulses(graph != null);
  const [live, setLive] = useState(false);
  useEffect(() => {
    const id = setInterval(() => setLive(Date.now() - lastFired.current < 2500), 500);
    return () => clearInterval(id);
  }, [lastFired]);

  useEffect(() => {
    fetchGraph().then(setGraph).catch((e) => setError(String(e)));
  }, []);

  const noFilter = useMemo(() => () => true, []);

  const runSearch = (q: string) => {
    if (!graph) return;
    const needle = q.trim().toLowerCase();
    if (!needle) return;
    const hit =
      graph.nodes.find((n) => n.label.toLowerCase() === needle) ??
      graph.nodes.find(
        (n) =>
          n.label.toLowerCase().includes(needle) || n.title.toLowerCase().includes(needle),
      );
    if (hit) {
      setSelected(hit);
      // new object each time so the graph's focus effect always re-fires
      setFocusLabel(hit.label + "#" + Date.now());
      setTimeout(() => setFocusLabel(hit.label), 0);
    }
  };

  if (error) {
    return (
      <div className="fatal">
        <h1>dream-web</h1>
        <p>Could not load the memory graph.</p>
        <pre>{error}</pre>
        <p className="muted">Is a sara database present? Set SARA_DB or pass --db.</p>
      </div>
    );
  }

  if (!graph) {
    return <div className="loading">loading memory graph…</div>;
  }

  return (
    <div className="app">
      <header className="topbar">
        <h1>
          dream<span>-web</span>
        </h1>
        <form
          className="search"
          onSubmit={(e) => {
            e.preventDefault();
            runSearch(search);
          }}
        >
          <input
            placeholder="jump to memory (label or title)…"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </form>
        <span className={`live ${live ? "on" : ""}`} title="pulses as agents recall memories">
          <span className="live-dot" /> live
        </span>
        <span className="db" title={graph.dbPath}>
          read-only
        </span>
      </header>

      <div className="main">
        <main className="canvas">
          <ViewToggles settings={view} onToggle={toggleView} />
          <Graph3D
            graph={graph}
            matches={noFilter}
            filterSig=""
            filtersActive={false}
            onSelect={setSelected}
            focusLabel={focusLabel}
            pulses={pulses}
            view={view}
          />
          <Ticker stream={stream} onPick={(label) => runSearch(label)} />
        </main>

        <SidePanel node={selected} onClose={() => setSelected(null)} />
      </div>
    </div>
  );
}
