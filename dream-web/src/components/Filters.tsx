import { projectColor } from "../color.ts";

interface Props {
  tags: string[];
  projects: string[];
  selectedTags: Set<string>;
  selectedProjects: Set<string>;
  onToggleTag: (t: string) => void;
  onToggleProject: (p: string) => void;
  onClear: () => void;
  counts: { nodes: number; edges: number; shown: number };
}

export function Filters({
  tags,
  projects,
  selectedTags,
  selectedProjects,
  onToggleTag,
  onToggleProject,
  onClear,
  counts,
}: Props) {
  const active = selectedTags.size > 0 || selectedProjects.size > 0;
  return (
    <div className="filters">
      <div className="filters-head">
        <h2>Filters</h2>
        {active && (
          <button className="clear" onClick={onClear}>
            clear
          </button>
        )}
      </div>
      <p className="counts">
        {active ? `${counts.shown} / ${counts.nodes}` : counts.nodes} memories · {counts.edges} links
      </p>

      <section>
        <h3>Projects</h3>
        <div className="chips">
          {projects.map((p) => (
            <button
              key={p}
              className={`chip ${selectedProjects.has(p) ? "on" : ""}`}
              onClick={() => onToggleProject(p)}
            >
              <span className="dot" style={{ background: projectColor(p) }} />
              {p}
            </button>
          ))}
          {projects.length === 0 && <span className="muted">none</span>}
        </div>
      </section>

      <section>
        <h3>Tags</h3>
        <div className="chips">
          {tags.map((t) => (
            <button
              key={t}
              className={`chip ${selectedTags.has(t) ? "on" : ""}`}
              onClick={() => onToggleTag(t)}
            >
              {t}
            </button>
          ))}
          {tags.length === 0 && <span className="muted">none</span>}
        </div>
      </section>
    </div>
  );
}
