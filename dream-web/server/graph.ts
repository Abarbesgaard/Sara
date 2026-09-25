import type { DatabaseSync } from "node:sqlite";

export interface GraphNode {
  id: string; // memory uuid
  label: string; // mNN
  title: string;
  tags: string[];
  projects: string[];
  provisional: boolean;
  canonical: boolean;
  derivedCount: number;
  strength: number; // proxy, see strengthProxy()
  recallCount: number;
  recentlyRecalled: boolean;
}

export interface GraphEdge {
  source: string;
  target: string;
  kind: "bond" | "tag";
  relation: string | null; // authored relation for bonds
  weight: number;
}

export interface Graph {
  generatedAt: string;
  dbPath: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
  tags: string[]; // all tags present, sorted
  projects: string[]; // all projects present, sorted
}

export interface MemoryDetail {
  label: string;
  title: string;
  body: string;
  status: string;
  tags: string[];
  projects: string[];
  tasks: { id: number | null; description: string; source: string }[];
  files: string[];
  createdAt: string;
  modifiedAt: string;
}

// Tags on more than this many memories are treated as hubs: too generic to link
// everyone together (it would collapse the graph into a hairball), so they are
// skipped for shared-tag adjacency. They remain visible as node tags and stay
// filterable.
const TAG_HUB_CAP = 12;

// Only memories in these statuses are visualized — archived/forgotten memories
// are excluded, mirroring what `recall` surfaces.
const VISIBLE_STATUSES = ["active", "provisional"];

interface Row {
  [key: string]: unknown;
}

function labelOf(displayId: number | null): string {
  return `m${displayId ?? 0}`;
}

/** Base 1.0 + recall boost (30d, +0.1/hit capped +0.5) + canonical bonus
 * (+0.1/derived child capped +0.5). A read-only approximation of sara's
 * `item_strength`, enough to size/glow nodes. */
function strengthProxy(recall30d: number, derivedCount: number): number {
  const recallBoost = Math.min(recall30d * 0.1, 0.5);
  const canonicalBonus = Math.min(derivedCount * 0.1, 0.5);
  return 1.0 + recallBoost + canonicalBonus;
}

export function buildGraph(db: DatabaseSync, dbPath: string): Graph {
  const memRows = db
    .prepare(
      `SELECT uuid, display_id, title, status
         FROM items
        WHERE kind = 'memory' AND status IN (${VISIBLE_STATUSES.map(() => "?").join(",")})`,
    )
    .all(...VISIBLE_STATUSES) as Row[];

  const memUuids = new Set(memRows.map((r) => String(r.uuid)));

  // Tags per memory.
  const tagsByUuid = new Map<string, string[]>();
  for (const r of db.prepare(`SELECT item_uuid, tag FROM item_tags`).all() as Row[]) {
    const u = String(r.item_uuid);
    if (!memUuids.has(u)) continue;
    (tagsByUuid.get(u) ?? tagsByUuid.set(u, []).get(u)!).push(String(r.tag));
  }

  // Projects per memory.
  const projectsByUuid = new Map<string, string[]>();
  for (const r of db.prepare(`SELECT item_uuid, project FROM item_projects`).all() as Row[]) {
    const u = String(r.item_uuid);
    if (!memUuids.has(u)) continue;
    (projectsByUuid.get(u) ?? projectsByUuid.set(u, []).get(u)!).push(String(r.project));
  }

  // Recall counts: all-time and last 7 days (for the pulse) and last 30 days
  // (for the strength proxy).
  const recallAll = new Map<string, number>();
  const recall7d = new Map<string, number>();
  const recall30d = new Map<string, number>();
  for (const r of db
    .prepare(
      `SELECT ref_uuid,
              COUNT(*) AS all_time,
              SUM(CASE WHEN at >= datetime('now','-7 days')  THEN 1 ELSE 0 END) AS d7,
              SUM(CASE WHEN at >= datetime('now','-30 days') THEN 1 ELSE 0 END) AS d30
         FROM events
        WHERE action = 'memory_recalled' AND ref_uuid IS NOT NULL
        GROUP BY ref_uuid`,
    )
    .all() as Row[]) {
    const u = String(r.ref_uuid);
    recallAll.set(u, Number(r.all_time ?? 0));
    recall7d.set(u, Number(r.d7 ?? 0));
    recall30d.set(u, Number(r.d30 ?? 0));
  }

  // derived_from edges define canonical/derived families: `from` derives from
  // `to`, so `to` is canonical and its derived-count is the number of incoming
  // derived_from edges.
  const derivedCount = new Map<string, number>();
  for (const r of db
    .prepare(`SELECT to_uuid, COUNT(*) AS n FROM memory_links WHERE relation = 'derived_from' GROUP BY to_uuid`)
    .all() as Row[]) {
    derivedCount.set(String(r.to_uuid), Number(r.n ?? 0));
  }

  const nodes: GraphNode[] = memRows.map((r) => {
    const uuid = String(r.uuid);
    const tags = (tagsByUuid.get(uuid) ?? []).sort();
    const projects = (projectsByUuid.get(uuid) ?? []).sort();
    const dc = derivedCount.get(uuid) ?? 0;
    const r30 = recall30d.get(uuid) ?? 0;
    return {
      id: uuid,
      label: labelOf(r.display_id as number | null),
      title: String(r.title ?? ""),
      tags,
      projects,
      provisional: r.status === "provisional",
      canonical: dc > 0,
      derivedCount: dc,
      strength: strengthProxy(r30, dc),
      recallCount: recallAll.get(uuid) ?? 0,
      recentlyRecalled: (recall7d.get(uuid) ?? 0) > 0,
    };
  });

  // --- Edges ---------------------------------------------------------------
  const edges: GraphEdge[] = [];
  const seen = new Set<string>();
  const pairKey = (a: string, b: string) => (a < b ? `${a}|${b}` : `${b}|${a}`);

  // Authored memory_links between two visible memories.
  for (const r of db
    .prepare(`SELECT from_uuid, to_uuid, relation, weight FROM memory_links`)
    .all() as Row[]) {
    const a = String(r.from_uuid);
    const b = String(r.to_uuid);
    if (!memUuids.has(a) || !memUuids.has(b) || a === b) continue;
    const key = pairKey(a, b);
    if (seen.has(key)) continue;
    seen.add(key);
    edges.push({
      source: a,
      target: b,
      kind: "bond",
      relation: String(r.relation),
      weight: Number(r.weight ?? 1),
    });
  }

  // Shared-tag adjacency, skipping hub tags. Rarer tags bind more tightly.
  const membersByTag = new Map<string, string[]>();
  for (const [uuid, tags] of tagsByUuid) {
    for (const tag of tags) {
      (membersByTag.get(tag) ?? membersByTag.set(tag, []).get(tag)!).push(uuid);
    }
  }
  for (const [, members] of membersByTag) {
    if (members.length < 2 || members.length > TAG_HUB_CAP) continue;
    const weight = 1 / Math.log2(members.length + 1); // IDF-ish: rarer => stronger
    for (let i = 0; i < members.length; i++) {
      for (let j = i + 1; j < members.length; j++) {
        const key = pairKey(members[i], members[j]);
        if (seen.has(key)) continue; // authored bond or an earlier tag edge wins
        seen.add(key);
        edges.push({
          source: members[i],
          target: members[j],
          kind: "tag",
          relation: null,
          weight,
        });
      }
    }
  }

  const allTags = [...membersByTag.keys()].sort();
  const allProjects = [
    ...new Set([...projectsByUuid.values()].flat()),
  ].sort();

  return {
    generatedAt: new Date().toISOString(),
    dbPath,
    nodes,
    edges,
    tags: allTags,
    projects: allProjects,
  };
}

/** Full memory body + linked tasks/files for the click-through side panel.
 * Purely a read — records NO recall event (unlike `sara dream`), so viewing
 * here never perturbs memory strength. */
export function getMemory(db: DatabaseSync, label: string): MemoryDetail | null {
  const m = /^m(\d+)$/.exec(label.trim());
  if (!m) return null;
  const displayId = Number(m[1]);

  const row = db
    .prepare(
      `SELECT uuid, title, body, status, created, modified
         FROM items
        WHERE kind = 'memory' AND display_id = ?
          AND status IN (${VISIBLE_STATUSES.map(() => "?").join(",")})
        ORDER BY CASE status WHEN 'active' THEN 0 ELSE 1 END
        LIMIT 1`,
    )
    .get(displayId, ...VISIBLE_STATUSES) as Row | undefined;
  if (!row) return null;
  const uuid = String(row.uuid);

  const tags = (db.prepare(`SELECT tag FROM item_tags WHERE item_uuid = ?`).all(uuid) as Row[])
    .map((r) => String(r.tag))
    .sort();
  const projects = (
    db.prepare(`SELECT project FROM item_projects WHERE item_uuid = ?`).all(uuid) as Row[]
  )
    .map((r) => String(r.project))
    .sort();
  const tasks = (
    db
      .prepare(
        `SELECT t.id AS id, t.description AS description, l.source AS source
           FROM item_task_links l JOIN tasks t ON t.uuid = l.task_uuid
          WHERE l.item_uuid = ?`,
      )
      .all(uuid) as Row[]
  ).map((r) => ({
    id: (r.id as number | null) ?? null,
    description: String(r.description ?? ""),
    source: String(r.source ?? "auto"),
  }));
  const files = (db.prepare(`SELECT file_path FROM item_files WHERE item_uuid = ?`).all(uuid) as Row[])
    .map((r) => String(r.file_path))
    .sort();

  return {
    label,
    title: String(row.title ?? ""),
    body: String(row.body ?? ""),
    status: String(row.status ?? ""),
    tags,
    projects,
    tasks,
    files,
    createdAt: String(row.created ?? ""),
    modifiedAt: String(row.modified ?? ""),
  };
}
