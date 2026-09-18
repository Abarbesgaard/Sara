export interface GraphNode {
  id: string;
  label: string;
  title: string;
  tags: string[];
  projects: string[];
  provisional: boolean;
  canonical: boolean;
  derivedCount: number;
  strength: number;
  recallCount: number;
  recentlyRecalled: boolean;
}

export interface GraphEdge {
  source: string;
  target: string;
  kind: "bond" | "tag";
  relation: string | null;
  weight: number;
}

export interface Graph {
  generatedAt: string;
  dbPath: string;
  nodes: GraphNode[];
  edges: GraphEdge[];
  tags: string[];
  projects: string[];
}

export interface Pulse {
  label: string;
  at: string;
}

export interface PulseFeed {
  now: string;
  pulses: Pulse[];
}

export type ActivityAction = "recall" | "surface" | "learn" | "link" | "task";

export interface ActivityPulse {
  label: string;
  action: ActivityAction;
  at: string;
}

export interface ActivityFeed {
  now: string;
  pulses: ActivityPulse[];
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

export async function fetchGraph(): Promise<Graph> {
  const res = await fetch("/api/graph");
  if (!res.ok) throw new Error(`/api/graph -> ${res.status}`);
  return res.json();
}

export async function fetchMemory(label: string): Promise<MemoryDetail> {
  const res = await fetch(`/api/memory/${encodeURIComponent(label)}`);
  if (!res.ok) throw new Error(`/api/memory/${label} -> ${res.status}`);
  return res.json();
}
