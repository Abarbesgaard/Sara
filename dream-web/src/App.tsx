import { useEffect, useMemo, useState } from "react";
import { fetchGraph, type Graph, type GraphNode } from "./api.ts";
import { Graph3D } from "./components/Graph3D.tsx";
import { Filters } from "./components/Filters.tsx";
import { SidePanel } from "./components/SidePanel.tsx";
import { usePulses } from "./usePulses.ts";

export function App() {
  const [graph, setGraph] = useState<Graph | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [selectedTags, setSelectedTags] = useState<Set<string>>(new Set());
  const [selectedProjects, setSelectedProjects] = useState<Set<string>>(new Set());
  const [selected, setSelected] = useState<GraphNode | null>(null);
  const [search, setSearch] = useState("");
  const [focusLabel, setFocusLabel] = useState<string | null>(null);
  const [filtersOpen, setFiltersOpen] = useState(true);

  // Live recall feed — pulse nodes as other processes recall them.
  const { pulses, lastFired } = usePulses(graph != null);
  const [live, setLive] = useState(false);
  useEffect(() => {
    const id = setInterval(() => setLive(Date.now() - lastFired.current < 2500), 500);
    return () => clearInterval(id);
  }, [lastFired]);

  useEffect(() => {
    fetchGraph().then(setGraph).catch((e) => setError(String(e)));
  }, []);

  const filtersActive = selectedTags.size > 0 || selectedProjects.size > 0;
  const filterSig = useMemo(
    () => `${[...selectedTags].sort().join(",")}|${[...selectedProjects].sort().join(",")}`,
    [selectedTags, selectedProjects],
  );

  const matches = useMemo(() => {
    return (n: GraphNode): boolean => {
      const tagOk = selectedTags.size === 0 || n.tags.some((t) => selectedTags.has(t));
      const projOk =
        selectedProjects.size === 0 || n.projects.some((p) => selectedProjects.has(p));
      return tagOk && projOk;
    };
  }, [selectedTags, selectedProjects]);

  const shownCount = useMemo(() => {
    if (!graph) return 0;
    if (!filtersActive) return graph.nodes.length;
    return graph.nodes.filter(matches).length;
  }, [graph, matches, filtersActive]);

  const toggle = (set: Set<string>, v: string): Set<string> => {
    const next = new Set(set);
    next.has(v) ? next.delete(v) : next.add(v);
    return next;
  };

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
        <Filters
          open={filtersOpen}
          onToggle={() => setFiltersOpen((o) => !o)}
          tags={graph.tags}
          projects={graph.projects}
          selectedTags={selectedTags}
          selectedProjects={selectedProjects}
          onToggleTag={(t) => setSelectedTags((s) => toggle(s, t))}
          onToggleProject={(p) => setSelectedProjects((s) => toggle(s, p))}
          onClear={() => {
            setSelectedTags(new Set());
            setSelectedProjects(new Set());
          }}
          counts={{ nodes: graph.nodes.length, edges: graph.edges.length, shown: shownCount }}
        />

        <main className="canvas">
          <Graph3D
            graph={graph}
            matches={matches}
            filterSig={filterSig}
            filtersActive={filtersActive}
            onSelect={setSelected}
            focusLabel={focusLabel}
            pulses={pulses}
          />
        </main>

        <SidePanel node={selected} onClose={() => setSelected(null)} />
      </div>
    </div>
  );
}
