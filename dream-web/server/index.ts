import express from "express";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { existsSync } from "node:fs";
import { resolveDbPath } from "./dbPath.ts";
import { openReadOnly } from "./db.ts";
import type { DatabaseSync } from "node:sqlite";
import { buildGraph, getMemory, recentRecalls, recentActivity } from "./graph.ts";

const HOST = "127.0.0.1"; // localhost-only: no data ever leaves the machine.
const PORT = Number(process.env.PORT ?? 7777);

const __dirname = dirname(fileURLToPath(import.meta.url));
const distDir = join(__dirname, "..", "dist");

const dbPath = resolveDbPath();
let db: DatabaseSync;
try {
  db = openReadOnly(dbPath);
} catch (err) {
  console.error(String(err instanceof Error ? err.message : err));
  process.exit(1);
}

const app = express();

app.get("/api/graph", (_req, res) => {
  try {
    res.json(buildGraph(db, dbPath));
  } catch (err) {
    res.status(500).json({ error: String(err instanceof Error ? err.message : err) });
  }
});

app.get("/api/memory/:label", (req, res) => {
  try {
    const detail = getMemory(db, req.params.label);
    if (!detail) {
      res.status(404).json({ error: `no memory ${req.params.label}` });
      return;
    }
    res.json(detail);
  } catch (err) {
    res.status(500).json({ error: String(err instanceof Error ? err.message : err) });
  }
});

app.get("/api/health", (_req, res) => {
  res.json({ ok: true, dbPath, readOnly: true });
});

// Live recall feed (legacy): which memories another process recalled since the
// last poll. Kept for compatibility; the richer feed is /api/activity.
app.get("/api/pulses", (req, res) => {
  try {
    const since = typeof req.query.since === "string" ? req.query.since : null;
    res.json(recentRecalls(db, since));
  } catch (err) {
    res.status(500).json({ error: String(err instanceof Error ? err.message : err) });
  }
});

// Unified brain-activity feed: every visible memory node that fired since the
// last poll, tagged with the MCP action (recall/surface/learn/link/task) that
// lit it. Read-only — derived from streams already on disk.
app.get("/api/activity", (req, res) => {
  try {
    const since = typeof req.query.since === "string" ? req.query.since : null;
    res.json(recentActivity(db, since));
  } catch (err) {
    res.status(500).json({ error: String(err instanceof Error ? err.message : err) });
  }
});

// Serve the built frontend in production (`npm start`). In dev, Vite serves the
// UI on :5173 and proxies /api here.
if (existsSync(distDir)) {
  app.use(express.static(distDir));
  app.get("*", (_req, res) => res.sendFile(join(distDir, "index.html")));
}

const server = app.listen(PORT, HOST, () => {
  console.log(`sara dream-web — read-only visualizer`);
  console.log(`  DB:  ${dbPath} (read-only)`);
  console.log(`  URL: http://${HOST}:${PORT}`);
  if (!existsSync(distDir)) {
    console.log(`  (dev: run 'vite' separately, or 'npm start' to serve the built UI here)`);
  }
});

function shutdown(signal: string) {
  console.log(`\n${signal} received — shutting down.`);
  server.close(() => {
    try {
      db.close();
    } catch {
      /* already closed */
    }
    process.exit(0);
  });
}
process.on("SIGINT", () => shutdown("SIGINT"));
process.on("SIGTERM", () => shutdown("SIGTERM"));
