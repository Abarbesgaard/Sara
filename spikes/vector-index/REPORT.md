# Spike: a vector index for sara memories

Task `442af1d1`. The question: should sara get a real vector index (sqlite-vec
locally, with a path to Postgres + pgvector for a shared team store), and what
would actually make recall better?

## TL;DR

| Idea | Verdict | Evidence |
|---|---|---|
| Fuse keyword and vector results with Reciprocal Rank Fusion; use strength as a weighted list, not the first sort key | **Go.** Biggest win, no new dependency | MRR@10 0.279 → 0.355 (+27%), hit@1 0.195 → 0.266 (+36%), ΔMRR 95% CI [+0.024, +0.129] |
| Embed the provisional memory `done` writes, and on `promote` | **Go (bug fix)** | 102/102 provisional memories created in October have no vector |
| Store vectors as an f32 BLOB instead of JSON text | **Go (cheap)** | 9–13× faster scan, 3× smaller, same results |
| sqlite-vec `vec0` | **No-go for now.** Revisit at ~10⁴ memories | Exact brute force (not ANN). 2.3× faster than BLOB at 10k–100k, +83 KB binary, but at today's 0.8k memories the whole semantic step costs ~1 ms of a 77 ms `recall` |
| Index task notes (findings, decisions) | **Defer.** Part of the shared-store work | 1,480 notes ≈ 1.5 MB; this memory-only eval can't measure the value |
| Shared team store (Postgres + pgvector) | **Design only** (below) | sara stays self-contained; the shared store is an opt-in sync target |

Speed is not sara's problem. **Ranking is.** Today's recall sorts by memory
strength *before* relevance and caps meaning-based hits at 5 appended results,
so semantic search adds only +0.011 MRR on top of keywords.

## What exists today

- `infrastructure/memory/embedding.rs`: a bundled Model2Vec static model
  (potion-8m int8, 256-dim, about 7.5 MB in the binary, no network). Vectors
  are L2-normalised means of token rows. A scheme fingerprint triggers a
  self-healing `reindex_all` when the model or the embed text changes.
- `embeddings(ref_uuid, vector_json TEXT)`: about 3.1 KB of JSON per vector.
- `recall/search/semantic.rs`: brute-force cosine over all active vectors
  (threshold 0.30, top 5). Hits not already found by FTS5 are **appended**.
- `recall/search/scoring.rs`: sorts by `exact_match`, then **strength**, then
  FTS rank, then cosine.

## Method

### Eval set (241 queries, mined from real use)

`mine_eval.py` builds it read-only from a sara DB. The data is personal and
work content, so it is **not committed**. Only aggregate numbers appear here.

| Source | n | Query | Relevant |
|---|---:|---|---|
| cited | 65 | a task's description | memories that task cited (`memory_uses.kind='cited'`) |
| linked | 158 | a memory's title | its `derived_from` / `similar_to` / `supersedes` targets (≤3 per target) |
| hand | 18 | a paraphrase that avoids the memory's own words | one memory |

Leakage control ("time travel"): a cited query may only retrieve memories
created before its task, and a linked query never retrieves itself. The
harness re-runs the FTS phrase → AND → OR cascade against that time-travelled
corpus. Without this, the phrase stage "succeeds" on the leaked documents,
never falls back to tokens, and keyword search scores 0. That was a real
pitfall caught during the spike.

Bias: cited memories were found by the current strength-first ranker, so
gains measured against it are **conservative**.

### Harness

`tests/unit/commands/recall/eval_spike.rs` is an `#[ignore]` test inside the
crate, so it uses the real `fts`, `embedding` and `db` code. It reports
recall@5/@10, MRR@10, hit@1, ms/query, and a paired bootstrap (10k resamples)
of ΔMRR against the current ranking.

## Results

### Experiment A: ranking (241 queries, snapshot 1)

| Variant | recall@5 | recall@10 | MRR@10 | hit@1 |
|---|---:|---:|---:|---:|
| 1. current (FTS + ≤5 semantic, strength-first) | 0.325 | 0.469 | 0.279 | 0.195 |
| 2. current, semantic off | 0.303 | 0.414 | 0.268 | 0.187 |
| 3. FTS rank only | 0.318 | 0.401 | 0.256 | 0.170 |
| 4. vector only | 0.306 | 0.426 | 0.280 | 0.207 |
| 5. RRF(fts, vector) | 0.382 | 0.487 | 0.328 | 0.220 |
| 7a. RRF(fts, vector, strength×0.25) | 0.402 | 0.508 | 0.344 | 0.241 |
| **7b. RRF(fts, vector, strength×0.5)** | 0.394 | 0.517 | **0.355** | **0.266** |
| 7c. RRF(fts, vector, strength×1.0) | 0.396 | 0.521 | 0.346 | 0.245 |

ΔMRR against variant 1, with 95% bootstrap CI:

| Variant | all (241) | cited (65, held out) | linked (158, tuning) |
|---|---|---|---|
| 5. RRF(fts, vector) | +0.049 [−0.007, +0.106] | +0.083 [−0.015, +0.184] | +0.029 [−0.040, +0.101] |
| 7a. strength×0.25 | +0.065 [+0.012, +0.118] | +0.112 [+0.015, +0.212] | +0.042 [−0.022, +0.107] |
| **7b. strength×0.5** | **+0.076 [+0.024, +0.129]** | **+0.140 [+0.044, +0.239]** | +0.045 [−0.016, +0.110] |
| 7c. strength×1.0 | +0.067 [+0.019, +0.116] | +0.116 [+0.026, +0.208] | +0.041 [−0.017, +0.099] |

A second, later DB snapshot gave ΔMRR +0.098 [+0.046, +0.150] for 7b. All
three strength weights are significant overall, so the result doesn't hinge
on tuning. Strength helps as a prior and hurts as the primary sort key.
Hand-written paraphrases favour vector-only (MRR 0.442 vs 0.245), which is
small n but a sign that meaning matters more than the current append allows.

Open observation (unverified): on the hand set, the real product path
(MRR 0.156) underperforms its own emulation without cluster collapse (0.245).
`collapse_clusters` may hide the exact memory behind its canonical. Worth a
follow-up.

### Experiment B: storage and latency (`bench/`)

256-dim unit vectors, Apple Silicon, p50 of 30 queries, top-10:

| N | json (today) | f32 BLOB | in-memory | sqlite-vec vec0 |
|---:|---:|---:|---:|---:|
| 1k | 5.4 ms / 4.1 MB | 0.41 ms / 1.4 MB | 0.09 ms | 0.16 ms / 1.1 MB |
| 10k | 56 ms / 41 MB | 5.9 ms / 14 MB | 0.96 ms | 2.5 ms / 11 MB |
| 100k | 556 ms / 411 MB | 60 ms / 137 MB | 9.9 ms | 26 ms / 105 MB |

- vec0 agrees 100% with exact top-10 because it *is* brute force (SIMD), not ANN.
- sqlite-vec 0.1.9 adds **83 KB** to a stripped `panic=abort` binary
  (`size_probe`). It links against rusqlite's bundled SQLite (`SQLITE_CORE`,
  registered via `sqlite3_auto_extension`).
- Real `sara recall` takes 77 ms wall. Process start (`sara --version`) is
  51 ms, and the semantic step is about 1 ms.
- Risks: musl (asg017/sqlite-vec#314; sara doesn't ship musl), and a
  maintenance gap in late 2025 (releases resumed in 2026). The open Windows
  MSVC issue (asg017/sqlite-vec#21) did not bite:
  `.github/workflows/spike-vector-bench.yml` builds and runs vec0 on Linux x64
  and arm64, macOS and Windows (CI p50 at 10k: 3.7–8.5 ms, top-10 agreement 1.00).

### Experiment C: coverage

- **Bug:** `done`'s auto-memory (`db/memory/maintenance.rs`, `insert_item`
  with status `provisional`) is never embedded, and `promote` doesn't embed
  either. On the live store, 0 of the 102 provisional memories created in
  October have a vector, so they are invisible to semantic recall even after
  promotion. This breaks the same invariant as the earlier relearn bug: every
  write path that creates or changes memory text must call
  `embedding::index_memory`.
- Re-embedding everything takes 125 ms wall (828 memories).
- With full coverage, MRR is within noise on this eval. The eval can't credit
  these memories, because a memory that is never surfaced is never cited.
  Treat this as a correctness fix.
- 1,480 task notes (799 findings, 252 decisions) are searchable by keyword
  only. Indexing them costs about 1.5 MB.

## Recommended follow-ups (in order)

1. **RRF ranking in `recall/search/collect.rs`.** Take the FTS list and the
   vector top-50 as ranked lists, plus a strength-ordered list at weight 0.5,
   with k = 60. Keep `exact_match` (filter hits) on top. Promote
   `eval_spike` into a regression gate with a frozen, anonymised fixture DB.
2. **Embed on every memory write path:** `done`'s auto-memory and `promote`.
   Add a test helper that asserts coverage. Remember that `db::insert_item`
   does not embed.
3. **Migrate `vector_json` to `vector BLOB`.** Bump the scheme version so
   `ensure_index_current` rebuilds the index itself, with no data migration.
4. Investigate whether cluster collapse hides exact matches.
5. Reconsider sqlite-vec (or an in-process cache in `sara mcp`) when a store
   passes ~10k memories.

## Design: path to a shared team store

Sara stays **self-contained**: the local SQLite DB is the source of truth and
sara never needs the network to work. The shared store is an opt-in sync
target, layered as the multi-agent design proposed:

```
 sara (each dev, local)                     shared (team, Postgres + pgvector)
 ┌──────────────────────────┐   push   ┌─────────────────────────────────────┐
 │ notes / memories         │ ───────▶ │ CAPTURE  append-only inbox           │
 │ outbox(event, sent_at)   │          │   (observation, author, repo, task)  │
 └──────────────────────────┘          │ FOLD     curator agent: dedupe,      │
            ▲                          │   link, flag contradictions → human  │
            │ pull (provisional)       │ STORE    canonical memories (diffable)│
            └───────────────────────── │ INDEX    pgvector + tsvector (derived,│
                                       │   rebuildable)                        │
                                       └─────────────────────────────────────┘
```

- **Outbox, not dual-write.** Writes go to the local DB and an `outbox` row in
  the same transaction. `sara sync` pushes later. If it's offline, nothing
  breaks.
- **Pull lands as `provisional`.** Team memories enter recall at provisional
  strength until promoted, so a bad memory from someone else can't outrank
  your own proven ones.
- **Vectors carry the scheme fingerprint.** Cosine is only meaningful between
  vectors from the same model and embed text (`embedding_scheme_version`).
  The server stores `(scheme, vector)` and either requires the same bundled
  model or re-embeds server-side. Vectors are derived data, so the payload
  that syncs is text.
- **One index trait, when there is a second backend:**
  `VectorIndex { upsert, delete, knn(query, k, filter), rebuild }` with
  in-process/BLOB (local) and pgvector (`vector(256)`, HNSW,
  `vector_cosine_ops`) implementations. Hybrid search on the server mirrors
  the local RRF: `tsvector` rank plus pgvector rank, fused the same way, so
  local and shared results feel the same.
- **Privacy:** sync is per project with an allowlist. Nothing syncs by
  default. Memories carry `project`, so work and personal projects never mix.

## Reproduce

```sh
# eval (from the repo root)
sqlite3 "file:$HOME/Library/Application Support/sara/tasks.db?mode=ro" ".backup /tmp/eval.db"
python3 spikes/vector-index/mine_eval.py > /tmp/eval.jsonl   # + optional hand-labelled lines
SARA_EVAL_DB=/tmp/eval.db SARA_EVAL_FILE=/tmp/eval.jsonl \
  cargo test --release eval_spike -- --ignored --nocapture

# storage bench and size probe
cd spikes/vector-index/bench
cargo run --release -- 1000 10000 100000
cargo build --release --bin size_probe && ls -l target/release/size_probe
cargo build --release --bin size_probe --no-default-features && ls -l target/release/size_probe
```

Use a fresh DB copy per run: recall records usage, which changes strengths.
