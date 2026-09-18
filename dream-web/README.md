# dream-web

A standalone, **read-only** web visualizer for your sara memory graph — a 3D,
force-directed "dream" of every active memory, the bonds you authored between
them, and the shared-tag clusters that pull related memories together.

It is a separate Node/TypeScript app that lives alongside the Rust `sara`
binary. It opens the sara SQLite database **read-only** and records **nothing**:
opening a memory here never touches recall counts or memory strength (unlike the
`sara dream` TUI, which records a recall on drill-in).

> Implements issue #147 — "Web-based 3D memory-cluster explorer".

## Quick start

```bash
cd dream-web
npm install
npm start
```

Then open <http://127.0.0.1:7777>. `npm start` builds the frontend and serves
both the UI and the API from a single localhost-only server.

## Development

```bash
npm run dev        # vite dev server + API with hot reload (concurrently)
npm run typecheck  # tsc for frontend + backend
npm run build      # production build into dist/
```

In dev, Vite serves the UI on its own port and proxies `/api/*` to the backend
on `127.0.0.1:7777`.

## Database location

By default dream-web auto-detects the sara database at the platform default
(on macOS: `~/Library/Application Support/sara/tasks.db`). Override it with
either:

```bash
SARA_DB=/path/to/tasks.db npm start
# or
npx tsx server/index.ts --db /path/to/tasks.db
```

The connection is opened with `node:sqlite`'s `readOnly: true`, so writes are
physically rejected by SQLite.

## What you see

- **Nodes** — one per visible memory (`active` or `provisional`; archived
  memories are excluded, mirroring what `recall` surfaces).
  - **Size** grows with a strength proxy (recent recalls + canonical family
    size).
  - **Colour** is keyed to the memory's project.
  - **Provisional** memories are dimmed/desaturated.
  - **Canonical** memories (those other memories derive from) carry a floating
    `⬡N` badge showing how many memories derive from them.
  - **Hover** for a tooltip (label, title, tags, projects, status).
- **Links**
  - **Blue** — authored `memory_links` bonds (relations like `derived_from`,
    `relates_to`).
  - **Grey** — shared-tag adjacency. Generic hub tags (on more than 12
    memories) are skipped so the graph doesn't collapse into a hairball; rarer
    shared tags bind more tightly.
- **Filters** (left) — narrow the view by project and/or tag; non-matching
  nodes fade back.
- **Search** (top) — jump the camera to a memory by label (`m42`) or title.
- **Side panel** (right) — click a node to read its full body plus linked
  tasks and files. This is a pure read; nothing is recorded.

## Ports & privacy

The server binds `127.0.0.1` only — nothing is exposed off the machine, and no
memory data ever leaves it. Port defaults to `7777` (override with `PORT`).

## Architecture

```
server/
  dbPath.ts   resolve the sara DB path (SARA_DB env > --db arg > platform default)
  db.ts       open the DB read-only (node:sqlite)
  graph.ts    build the node/edge graph; drill into a single memory
  index.ts    Express: /api/graph, /api/memory/:label, /api/health; serve dist/
src/
  api.ts      typed client + shared graph/memory interfaces
  color.ts    deterministic per-project colours
  App.tsx     layout: filters + 3D canvas + side panel, search & filter state
  components/
    Graph3D.tsx   react-force-graph-3d wiring & node/link encoding
    Filters.tsx   project/tag chips
    SidePanel.tsx click-through memory detail
```

The browser's force simulation handles layout; the backend just pulls raw nodes
and edges from the DB (it deliberately does **not** replicate sara's calibrated
Rust weighting).
