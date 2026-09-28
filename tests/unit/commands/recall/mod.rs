use super::*;
use crate::infrastructure::config::Config;
use crate::infrastructure::model::{Status, Task};

fn cfg() -> Config {
    Config::default()
}

fn seed_memory(
    conn: &Connection,
    title: &str,
    body: &str,
    tags: &[&str],
    projects: &[&str],
) -> Item {
    let mut item = Item::new_memory(title.to_string(), body.to_string(), None);
    item.tags = tags.iter().map(|t| t.to_string()).collect();
    item.path = Some(String::new());
    db::insert_item(conn, &mut item).unwrap();
    db::set_item_projects(
        conn,
        &item.uuid,
        &projects.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
    )
    .unwrap();
    item
}

/// A Config with semantic recall enabled, for the semantic-path tests.
fn cfg_semantic() -> Config {
    let mut c = Config::default();
    c.recall.semantic = true;
    c
}

#[test]
fn recall_semantic_surfaces_paraphrase() {
    // The core value of the embedder: a query that shares NO literal token
    // with a memory still surfaces it via embedding cosine — something the
    // lexical FTS path (validated in the same test) cannot do.
    let conn = db::open_in_memory_for_test();
    let m = seed_memory(
        &conn,
        "CI restore step failed",
        "a dependabot bump broke the build; pin the lockfile version to fix it",
        &[],
        &[],
    );
    crate::infrastructure::embedding::index_memory(&conn, &m);

    // A paraphrase with no shared content word ("automated" "dependency"
    // "update" "wrecked" "pipeline" vs "dependabot" "bump" "broke" "build").
    let query = "automated dependency update wrecked the pipeline";

    // Lexical-only recall misses it (no literal overlap).
    let lexical = collect_hits(&conn, query, &[], &[], &[], 20, &SemanticOpts::off()).unwrap();
    assert!(
        lexical.is_empty(),
        "lexical recall should NOT surface the paraphrase, got {} hits",
        lexical.len()
    );

    // Semantic recall surfaces it, flagged as a semantic hit with a cosine.
    let semantic = collect_hits(
        &conn,
        query,
        &[],
        &[],
        &[],
        20,
        &SemanticOpts::from_cfg(&cfg_semantic()),
    )
    .unwrap();
    assert_eq!(
        semantic.len(),
        1,
        "semantic recall should surface the memory"
    );
    assert!(semantic[0].semantic, "hit must be flagged semantic");
    assert!(
        semantic[0].cosine.unwrap() > 0.30,
        "cosine {:?} should clear the threshold",
        semantic[0].cosine
    );
}

#[test]
fn recall_semantic_respects_exact_tag_filter() {
    // A --tag filter is an AND constraint. Semantic recall must stay inside
    // it: a paraphrase-matching memory that lacks the tag must NOT surface,
    // or the filter silently leaks unrelated-tag memories.
    let conn = db::open_in_memory_for_test();

    // The memory the query paraphrases — but it is NOT tagged "ci".
    let untagged = seed_memory(
        &conn,
        "CI restore step failed",
        "a dependabot bump broke the build; pin the lockfile version to fix it",
        &[],
        &[],
    );
    crate::infrastructure::embedding::index_memory(&conn, &untagged);

    // An unrelated memory that DOES carry the "ci" tag.
    let tagged = seed_memory(
        &conn,
        "unrelated note",
        "remember to water the office plants on fridays",
        &["ci"],
        &[],
    );
    crate::infrastructure::embedding::index_memory(&conn, &tagged);

    let query = "automated dependency update wrecked the pipeline";
    let hits = collect_hits(
        &conn,
        query,
        &["ci".to_string()],
        &[],
        &[],
        20,
        &SemanticOpts::from_cfg(&cfg_semantic()),
    )
    .unwrap();

    assert!(
        hits.iter().all(|h| h.item_uuid != Some(untagged.uuid)),
        "semantic recall must not leak a memory outside the --tag filter"
    );
}

#[test]
fn recall_semantic_filter_applied_before_top_k_truncation() {
    // Regression: the exact-filter allowlist must be applied BEFORE the
    // `top_k` truncation. Here a higher-scoring memory sits OUTSIDE the
    // filter and a weaker one sits INSIDE it, with top_k = 1. If the filter
    // ran after truncation, the single slot would be consumed by the
    // out-of-filter memory and then discarded, leaving the valid in-filter
    // hit invisible.
    let conn = db::open_in_memory_for_test();

    let query = "lockfile pin fixes the broken dependabot build";

    // Strongest match, but NOT tagged "keep" — must never occupy the slot.
    let outside = seed_memory(
        &conn,
        "outside filter",
        "lockfile pin fixes the broken dependabot build",
        &[],
        &[],
    );
    crate::infrastructure::embedding::index_memory(&conn, &outside);

    // Weaker match, but carries the "keep" tag — this is the one we want.
    let inside = seed_memory(&conn, "inside filter", "lockfile pin broke", &["keep"], &[]);
    crate::infrastructure::embedding::index_memory(&conn, &inside);

    let opts = SemanticOpts {
        enabled: true,
        threshold: 0.10,
        top_k: 1,
    };
    let hits = collect_hits(&conn, query, &["keep".to_string()], &[], &[], 20, &opts).unwrap();

    assert!(
        hits.iter().any(|h| h.item_uuid == Some(inside.uuid)),
        "the in-filter memory must survive top_k truncation"
    );
    assert!(
        hits.iter().all(|h| h.item_uuid != Some(outside.uuid)),
        "the out-of-filter memory must never surface"
    );
}

#[test]
fn recall_full_body_reaches_agent_json_untruncated() {
    let conn = db::open_in_memory_for_test();
    let long_body = format!(
        "uniquewidget {}",
        "the full memory body must reach the agent intact. ".repeat(6)
    );
    assert!(
        long_body.chars().count() > 160,
        "the test body must exceed the 160-char preview cap"
    );
    let item = seed_memory(&conn, "long memory", &long_body, &[], &[]);

    // JSON path (the MCP recall tool / `--json`): the agent must receive the
    // full, untruncated memory body.
    let v = recall_value(&conn, &cfg(), "uniquewidget", &[], &[], &[], 20, false).unwrap();
    let hits = v["keyword"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "the memory should surface");
    assert_eq!(
        hits[0]["text"].as_str().unwrap(),
        long_body,
        "recall JSON must carry the complete memory body, not a 160-char preview"
    );

    // The human terminal view still gets a short preview.
    let hit = item_hit(&conn, item, false);
    assert!(
        hit.snippet.chars().count() <= 160,
        "terminal preview must stay capped, got {}",
        hit.snippet.chars().count()
    );
}

#[test]
fn recall_returns_only_the_top_hit_in_full_rest_as_guide() {
    let conn = db::open_in_memory_for_test();
    // Two memories that both match the query. Distinct long bodies so a
    // full-vs-preview mistake is unambiguous.
    seed_memory(
        &conn,
        "widget alpha",
        &format!("alpha body {}", "aaaaa ".repeat(40)),
        &[],
        &["p"],
    );
    seed_memory(
        &conn,
        "widget beta",
        &format!("beta body {}", "bbbbb ".repeat(40)),
        &[],
        &["p"],
    );

    let v = recall_value(&conn, &cfg(), "widget body", &[], &[], &[], 10, false).unwrap();
    let hits = v["keyword"].as_array().unwrap();
    assert!(hits.len() >= 2, "both memories should surface: {hits:?}");
    // The top hit carries the full body...
    assert!(
        hits[0]["text"].as_str().is_some_and(|t| t.len() > 160),
        "top hit must carry its full untruncated body"
    );
    // ...every other hit is a guide entry: preview present, no full text.
    for h in &hits[1..] {
        assert!(h["text"].is_null(), "non-top hits carry no full body: {h}");
        assert!(
            h["preview"].as_str().is_some(),
            "guide entries still carry a preview and label"
        );
        assert!(h["label"].as_str().is_some());
        // Guide entries must be LEAN: none of the bulky/null scaffolding
        // fields the top hit carries. Only signal-bearing keys survive.
        for noise in [
            "description",
            "ref_kind",
            "exact_match",
            "loose",
            "semantic",
            "cosine",
            "modified",
            "files",
            "provisional",
            "derived_count",
            "derived_children",
        ] {
            assert!(
                h.get(noise).is_none(),
                "guide entry must not carry `{noise}`: {h}"
            );
        }
    }
}

#[test]
fn recall_by_label_drills_into_a_single_memory_in_full() {
    let conn = db::open_in_memory_for_test();
    let seed = seed_memory(
        &conn,
        "canonical fix",
        &format!("the canonical mend {}", "detail ".repeat(40)),
        &[],
        &["p"],
    );
    let neighbour = seed_memory(
        &conn,
        "related note",
        "a related sibling in the cluster",
        &[],
        &["p"],
    );
    db::insert_memory_link(
        &conn,
        &seed.uuid.to_string(),
        &neighbour.uuid.to_string(),
        "similar_to",
        1.0,
    )
    .unwrap();
    let label = format!("m{}", seed.display_id.unwrap());

    // Recalling the bare handle returns that memory in full + its cluster guide.
    let v = recall_value(&conn, &cfg(), &label, &[], &[], &[], 10, false).unwrap();
    assert_eq!(v["deep"], true, "label recall is a deep drill-in");
    assert_eq!(v["spread"], "label");
    let hits = v["keyword"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "exactly the resolved memory");
    assert!(
        hits[0]["text"].as_str().is_some_and(|t| t.len() > 160),
        "the drilled-into memory comes back in full"
    );
    // Its cluster is a guide (no full bodies).
    let assoc = v["associative"].as_array().unwrap();
    assert!(
        assoc
            .iter()
            .any(|a| a["label"].as_str() == Some(&format!("m{}", neighbour.display_id.unwrap()))),
        "the linked neighbour appears in the cluster guide: {assoc:?}"
    );
    assert!(
        assoc.iter().all(|a| a["text"].is_null()),
        "cluster guide carries no full text"
    );
}

#[test]
fn recall_default_surfaces_semantic_paraphrase() {
    // Semantic recall is ALWAYS on: a paraphrase sharing no literal keyword
    // with any memory is surfaced by embedding similarity, even with a
    // default config and no --semantic flag.
    let conn = db::open_in_memory_for_test();
    let m = seed_memory(
        &conn,
        "CI restore step failed",
        "a dependabot bump broke the build; pin the lockfile version",
        &[],
        &[],
    );
    crate::infrastructure::embedding::index_memory(&conn, &m);

    let query = "automated dependency update wrecked the pipeline";
    let v = recall_value(&conn, &cfg(), query, &[], &[], &[], 20, false).unwrap();
    let hits = v["keyword"].as_array().unwrap();
    assert!(
        !hits.is_empty(),
        "always-on semantic recall must surface the paraphrase by default"
    );
    assert!(
        hits.iter()
            .any(|h| h["semantic"] == serde_json::json!(true)),
        "the paraphrase must be flagged as a semantic hit"
    );
}

#[test]
fn semantic_opts_from_cfg_is_always_enabled() {
    // Even when the legacy config toggle is off, from_cfg reports enabled —
    // semantic recall can no longer be turned off via config.
    let mut c = Config::default();
    c.recall.semantic = false;
    assert!(super::SemanticOpts::from_cfg(&c).enabled);
}

#[test]
fn recall_falls_back_to_token_and_when_phrase_misses() {
    let conn = db::open_in_memory_for_test();
    seed_memory(
        &conn,
        "MudTable pagination gotcha",
        "MudBlazor MudTable runs pagination server-side when using ServerData",
        &[],
        &["web-app"],
    );

    // Word order differs from the stored text → phrase literal misses,
    // token-AND fallback should still surface the memory.
    let hits = collect_hits(
        &conn,
        "pagination MudTable",
        &[],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].description, "MudTable pagination gotcha");
    assert!(!hits[0].exact_match);
}

#[test]
fn recall_surfaces_provisional_memories_with_flag() {
    let conn = db::open_in_memory_for_test();
    let mut item = Item::new_memory(
        "auto memory".to_string(),
        "synthesised frobnicator pattern".to_string(),
        None,
    );
    item.status = "provisional".to_string();
    item.path = Some(String::new());
    db::insert_item(&conn, &mut item).unwrap();

    let hits = collect_hits(
        &conn,
        "frobnicator",
        &[],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(hits[0].provisional, "provisional flag must be set");
}

#[test]
fn recall_auto_spreads_on_thin_hits_and_reports_mode() {
    let conn = db::open_in_memory_for_test();
    let seed = seed_memory(
        &conn,
        "redis eviction policy",
        "set maxmemory-policy allkeys-lru",
        &[],
        &["web-app"],
    );
    let neighbour = seed_memory(
        &conn,
        "cache stampede guard",
        "add a mutex around cold-cache fills",
        &[],
        &["web-app"],
    );
    db::insert_memory_link(
        &conn,
        &seed.uuid.to_string(),
        &neighbour.uuid.to_string(),
        "similar_to",
        1.0,
    )
    .unwrap();

    // Thin literal result (1 direct hit) now AUTO-spreads without --spread:
    // the linked neighbour surfaces associatively and the mode is "auto".
    let v = recall_value(&conn, &cfg(), "maxmemory-policy", &[], &[], &[], 20, false).unwrap();
    assert_eq!(v["keyword"].as_array().unwrap().len(), 1);
    assert_eq!(v["spread"], "auto", "one thin hit triggers auto-spread");
    assert!(
        v["associative"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["preview"].as_str().unwrap().contains("mutex")),
        "auto-spread should surface the linked neighbour"
    );

    // With explicit --spread: same surfacing, but the mode is "explicit".
    let v = recall_value(&conn, &cfg(), "maxmemory-policy", &[], &[], &[], 20, true).unwrap();
    assert_eq!(v["spread"], "explicit");
    let assoc = v["associative"].as_array().unwrap();
    assert!(
        assoc
            .iter()
            .any(|a| a["preview"].as_str().unwrap().contains("mutex")),
        "spread should surface the linked neighbour in the associative array"
    );
    assert!(
        assoc.iter().all(|a| a["text"].is_null()),
        "the associative array is a guide — it carries no full body text"
    );
    assert!(
        assoc.iter().all(|a| a["activation"].is_number()),
        "each associative hit carries an activation score"
    );
    assert!(
        assoc
            .iter()
            .all(|a| a["via"].as_array().is_some_and(|p| !p.is_empty())),
        "each associative hit carries a non-empty 'via' synaptic path"
    );
}

#[test]
fn recall_plentiful_hits_stay_lexical_no_auto_spread() {
    let conn = db::open_in_memory_for_test();
    // Three memories all match "cache" directly → not thin → no auto-spread.
    let a = seed_memory(
        &conn,
        "cache one",
        "cache eviction note one",
        &[],
        &["web-app"],
    );
    let b = seed_memory(
        &conn,
        "cache two",
        "cache warming note two",
        &[],
        &["web-app"],
    );
    seed_memory(
        &conn,
        "cache three",
        "cache stampede note three",
        &[],
        &["web-app"],
    );
    // A linked outsider that auto-spread WOULD surface if it fired.
    let outsider = seed_memory(
        &conn,
        "outsider",
        "unrelated mutex trick",
        &[],
        &["web-app"],
    );
    db::insert_memory_link(
        &conn,
        &a.uuid.to_string(),
        &outsider.uuid.to_string(),
        "similar_to",
        1.0,
    )
    .unwrap();
    db::insert_memory_link(
        &conn,
        &b.uuid.to_string(),
        &outsider.uuid.to_string(),
        "similar_to",
        1.0,
    )
    .unwrap();

    let v = recall_value(&conn, &cfg(), "cache", &[], &[], &[], 20, false).unwrap();
    assert!(
        v["keyword"].as_array().unwrap().len() >= 3,
        "plentiful direct hits"
    );
    assert_eq!(v["spread"], "off", "plentiful literal hits stay lexical");
    assert!(v["associative"].as_array().unwrap().is_empty());
}

#[test]
fn recall_zero_hits_does_not_spread() {
    let conn = db::open_in_memory_for_test();
    seed_memory(
        &conn,
        "unrelated",
        "nothing to do with the query",
        &[],
        &["web-app"],
    );

    let v = recall_value(&conn, &cfg(), "zzzznomatchqqq", &[], &[], &[], 20, false).unwrap();
    assert!(
        v["keyword"].as_array().unwrap().is_empty(),
        "no direct hits"
    );
    assert_eq!(v["spread"], "off", "zero hits cannot seed spreading");
    assert!(v["associative"].as_array().unwrap().is_empty());
}

#[test]
fn recall_bare_tag_lookup_does_not_auto_spread() {
    let conn = db::open_in_memory_for_test();
    // A single tag-matched memory (thin) linked to an outsider. A bare
    // tag lookup is already precise, so it must NOT auto-radiate even
    // though the hit count (1) is in the thin band.
    let a = seed_memory(
        &conn,
        "billing note",
        "billing invoice edge case",
        &["billing"],
        &["web-app"],
    );
    let outsider = seed_memory(
        &conn,
        "outsider",
        "unrelated mutex trick",
        &[],
        &["web-app"],
    );
    db::insert_memory_link(
        &conn,
        &a.uuid.to_string(),
        &outsider.uuid.to_string(),
        "similar_to",
        1.0,
    )
    .unwrap();

    // Empty query + tag filter → bare lookup.
    let v = recall_value(
        &conn,
        &cfg(),
        "",
        &["billing".to_string()],
        &[],
        &[],
        20,
        false,
    )
    .unwrap();
    assert_eq!(
        v["keyword"].as_array().unwrap().len(),
        1,
        "one thin tag hit"
    );
    assert_eq!(v["spread"], "off", "a bare tag lookup never auto-spreads");
    assert!(v["associative"].as_array().unwrap().is_empty());
}

#[test]
fn spreading_related_surfaces_linked_neighbour_not_in_direct_hits() {
    let conn = db::open_in_memory_for_test();
    // Direct hit on the query, and a linked neighbour that shares no query term.
    let seed = seed_memory(
        &conn,
        "postgres connection pooling",
        "use pgbouncer in front of postgres",
        &[],
        &["web-app"],
    );
    let neighbour = seed_memory(
        &conn,
        "supavisor tuning",
        "raise pool_size for burst traffic",
        &[],
        &["web-app"],
    );
    db::insert_memory_link(
        &conn,
        &seed.uuid.to_string(),
        &neighbour.uuid.to_string(),
        "similar_to",
        1.0,
    )
    .unwrap();

    let hits = collect_hits(&conn, "pgbouncer", &[], &[], &[], 20, &SemanticOpts::off()).unwrap();
    assert_eq!(hits.len(), 1, "only the seed matches the query directly");

    let related = spreading_related(&conn, &hits).unwrap();
    assert!(
        related.iter().any(|r| r.item.uuid == neighbour.uuid),
        "spreading activation should surface the linked neighbour"
    );
    assert!(
        related.iter().all(|r| r.item.uuid != seed.uuid),
        "direct hits must not be repeated in the associative section"
    );
    // The neighbour's path explains the hop: it ends at the neighbour and
    // starts at the seed memory.
    let n = related
        .iter()
        .find(|r| r.item.uuid == neighbour.uuid)
        .unwrap();
    let seed_lbl = format!("m{}", seed.display_id.unwrap_or(0));
    let neighbour_lbl = format!("m{}", neighbour.display_id.unwrap_or(0));
    assert_eq!(n.path.first(), Some(&seed_lbl));
    assert_eq!(n.path.last(), Some(&neighbour_lbl));
}

#[test]
fn spread_surfacing_does_not_reinforce_strength() {
    let conn = db::open_in_memory_for_test();
    // A seed that matches the query directly, and a neighbour that only
    // surfaces by spreading activation (shares no query term).
    let seed = seed_memory(
        &conn,
        "postgres connection pooling",
        "use pgbouncer in front of postgres",
        &[],
        &["web-app"],
    );
    let neighbour = seed_memory(
        &conn,
        "supavisor tuning",
        "raise pool_size for burst traffic",
        &[],
        &["web-app"],
    );
    db::insert_memory_link(
        &conn,
        &seed.uuid.to_string(),
        &neighbour.uuid.to_string(),
        "similar_to",
        1.0,
    )
    .unwrap();

    // The neighbour has never been deliberately recalled — baseline strength.
    assert_eq!(db::item_strength(&conn, &neighbour), 1.0);

    // A deliberate recall for the seed that radiates to the neighbour.
    let v = recall_value(&conn, &cfg(), "pgbouncer", &[], &[], &[], 20, true).unwrap();
    assert!(
        v["associative"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["preview"].as_str() == Some("raise pool_size for burst traffic")),
        "the neighbour must surface via spreading activation for this test to be meaningful"
    );

    // The neighbour surfaced ONLY as an uninvited graph side-effect. It must
    // NOT gain strength the way a deliberately-recalled memory does, or the
    // ranking signal is polluted by associative noise.
    assert_eq!(
        db::item_strength(&conn, &neighbour),
        1.0,
        "a spread-only surfacing must not reinforce strength like a deliberate recall"
    );
}

#[test]
fn spreading_related_is_empty_without_memory_seeds() {
    let conn = db::open_in_memory_for_test();
    // No memory hits → no seeds → nothing to spread from.
    let related = spreading_related(&conn, &[]).unwrap();
    assert!(related.is_empty());
}

#[test]
fn associative_output_is_capped_and_normalized() {
    let conn = db::open_in_memory_for_test();
    let hub = seed_memory(&conn, "hub topic", "central memory", &["hub"], &["p"]);
    // Many neighbours linked to the seed — without a cap, all would surface.
    for i in 0..12 {
        let n = seed_memory(&conn, &format!("n{i}"), &format!("body {i}"), &[], &["p"]);
        db::insert_memory_link(
            &conn,
            &hub.uuid.to_string(),
            &n.uuid.to_string(),
            "similar_to",
            1.0,
        )
        .unwrap();
    }
    let hits = collect_hits(&conn, "central", &[], &[], &[], 20, &SemanticOpts::off()).unwrap();
    let related = spreading_related(&conn, &hits).unwrap();

    assert!(
        related.len() <= 5,
        "associative results must be capped to ~5, got {}",
        related.len()
    );
    assert!(
        related.iter().all(|r| r.activation <= 1.0 + 1e-9),
        "activation must be normalized to <= 1.0"
    );
    assert!(
        related.iter().any(|r| r.activation >= 0.999),
        "the strongest associative hit must normalize to 1.0"
    );
}

#[test]
fn recall_by_tag_returns_only_matching_items() {
    let conn = db::open_in_memory_for_test();
    seed_memory(
        &conn,
        "about service-a",
        "how to call service-a",
        &["service-a"],
        &["web-app"],
    );
    seed_memory(
        &conn,
        "about service-b",
        "how to call service-b",
        &["service-b"],
        &["web-app"],
    );

    let hits = collect_hits(
        &conn,
        "",
        &["service-a".to_string()],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].description, "about service-a");
    assert!(hits[0].exact_match);
}

#[test]
fn recall_with_no_memories_and_no_tasks_reports_none_recorded() {
    let conn = db::open_in_memory_for_test();
    let v = recall_value(
        &conn,
        &cfg(),
        "",
        &["service-a".to_string()],
        &[],
        &[],
        20,
        false,
    )
    .unwrap();
    assert!(v["keyword"].as_array().unwrap().is_empty());
    assert!(!db::has_any_memories(&conn).unwrap());
}

#[test]
fn strong_linked_memory_outranks_plain_fts_hit_for_same_query() {
    let conn = db::open_in_memory_for_test();

    let mut task = Task::new("fix service-a auth bug".to_string(), "Sara".to_string());
    task.status = Status::Completed;
    db::insert_task(&conn, &mut task).unwrap();

    let mut linked = Item::new_memory(
        "service-a auth fix".to_string(),
        "service-a needs an X-Client-Id header".to_string(),
        Some(task.uuid),
    );
    linked.path = Some(String::new());
    db::insert_item(&conn, &mut linked).unwrap();

    let mut unlinked = Item::new_memory(
        "unrelated service-a note".to_string(),
        "service-a has a staging endpoint too".to_string(),
        None,
    );
    unlinked.path = Some(String::new());
    db::insert_item(&conn, &mut unlinked).unwrap();

    let hits = collect_hits(&conn, "service-a", &[], &[], &[], 20, &SemanticOpts::off()).unwrap();
    assert!(!hits.is_empty());
    assert_eq!(hits[0].description, "service-a auth fix");
}

#[test]
fn hyphenated_tag_is_not_misparsed_as_fts_operator() {
    let conn = db::open_in_memory_for_test();
    seed_memory(
        &conn,
        "hyphen tag test",
        "notes about service-a",
        &["service-a"],
        &[],
    );

    // Regression: FTS5 treats bare hyphens as NOT; --tag must never go
    // through MATCH at all (find_items_by_tag uses a plain WHERE clause),
    // and a free-text query containing a hyphen must still be quoted.
    let hits = collect_hits(
        &conn,
        "",
        &["service-a".to_string()],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);

    let hits = collect_hits(&conn, "service-a", &[], &[], &[], 20, &SemanticOpts::off()).unwrap();
    assert_eq!(hits.len(), 1);
}

#[test]
fn ties_on_strength_and_match_kind_break_by_recency() {
    let conn = db::open_in_memory_for_test();
    let older = seed_memory(
        &conn,
        "older note",
        "shared-topic body",
        &["shared-topic"],
        &[],
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    let newer = seed_memory(
        &conn,
        "newer note",
        "shared-topic body",
        &["shared-topic"],
        &[],
    );
    assert!(newer.modified >= older.modified);

    let hits = collect_hits(
        &conn,
        "",
        &["shared-topic".to_string()],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].description, "newer note");
}

#[test]
fn token_or_fallback_surfaces_partial_overlap_and_flags_it_loose() {
    // A memory about pruning. The query shares SOME terms ("pruning",
    // "decay") but adds terms the memory lacks ("archive", "policy" absent
    // from body) so token-AND requires all four and misses. The Tier-3
    // token-OR fallback should still surface it, flagged loose / low.
    let conn = db::open_in_memory_for_test();
    seed_memory(
        &conn,
        "prune stale entries",
        "pruning removes memories that decay over time",
        &[],
        &["engine"],
    );

    // token-AND over all four tokens finds nothing (memory lacks
    // "archive"/"garbage"), so recall would previously be empty.
    let and_only = db::search_fts_tokens(
        &conn,
        &[
            "pruning".into(),
            "decay".into(),
            "archive".into(),
            "garbage".into(),
        ],
        50,
    )
    .unwrap();
    assert!(and_only.is_empty(), "precondition: token-AND misses");

    let hits = collect_hits(
        &conn,
        "pruning decay archive garbage",
        &[],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(
        hits.len(),
        1,
        "OR fallback surfaces the partial-overlap memory"
    );
    assert_eq!(hits[0].description, "prune stale entries");
    assert!(hits[0].loose, "OR-fallback hit must be flagged loose");

    // Confidence must reflect the loose match, not medium/high.
    let (confidence, caveat) = match_confidence("pruning decay archive garbage", &[], &hits);
    assert_eq!(confidence, "low");
    assert!(caveat.contains("Loose match"));
}

#[test]
fn token_or_fallback_does_not_fire_when_stricter_tiers_match() {
    // When token-AND (or phrase) already matches, the loose fallback must
    // NOT fire and hits must NOT be flagged loose (no displacement).
    let conn = db::open_in_memory_for_test();
    seed_memory(
        &conn,
        "kafka consumer lag",
        "monitor consumer group lag with burrow",
        &[],
        &["platform"],
    );
    // Both tokens present → token-AND matches → not loose.
    let hits = collect_hits(
        &conn,
        "consumer lag",
        &[],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert!(!hits[0].loose, "token-AND hit must not be flagged loose");
    let (confidence, _) = match_confidence("consumer lag", &[], &hits);
    assert_eq!(confidence, "medium");
}

#[test]
fn bare_recall_returns_recent_memories_instead_of_erroring() {
    // A `sara recall` with no query/tag/project/file must not error — it
    // surfaces the most recent memories so exploratory recall just works.
    let conn = db::open_in_memory_for_test();
    seed_memory(&conn, "alpha note", "first insight", &[], &["proj"]);
    seed_memory(&conn, "beta note", "second insight", &[], &["proj"]);

    let v = recall_value(&conn, &cfg(), "", &[], &[], &[], 20, false).unwrap();
    assert_eq!(v["confidence"], "recent");
    assert_eq!(v["recent"], true);
    let keyword = v["keyword"].as_array().unwrap();
    assert_eq!(keyword.len(), 2, "both memories surface as recent hits");
}

#[test]
fn recall_value_keyword_and_confidence_are_stable() {
    // Pins the observable recall_value contract that must survive the
    // removal of the dead semantic scaffold: a matching free-text query
    // yields its keyword hit and a medium confidence signal.
    let conn = db::open_in_memory_for_test();
    seed_memory(
        &conn,
        "kafka consumer lag",
        "monitor consumer group lag with burrow",
        &[],
        &["platform"],
    );
    let v = recall_value(&conn, &cfg(), "burrow", &[], &[], &[], 20, false).unwrap();
    let keyword = v["keyword"].as_array().unwrap();
    assert_eq!(
        keyword.len(),
        1,
        "the matching memory surfaces as a keyword hit"
    );
    assert_eq!(keyword[0]["description"], "kafka consumer lag");
    assert_eq!(
        v["confidence"], "medium",
        "free-text FTS hit => medium confidence"
    );
}

#[test]
fn bm25_relevance_orders_equal_strength_fts_hits_over_recency() {
    let conn = db::open_in_memory_for_test();
    // Strong bm25 match: term-dense, short document. Plain memory (1.0).
    let relevant = seed_memory(&conn, "widget widget widget", "widget", &[], &[]);
    std::thread::sleep(std::time::Duration::from_millis(20));
    // Weak bm25 match: the term appears once in a long, mostly-unrelated
    // body. Inserted later, so it is MORE RECENT — under the old recency
    // tie-break it wrongly led despite being the poorer match.
    let recent_but_weak = seed_memory(
        &conn,
        "misc note",
        "this is a long note about many unrelated things and it merely \
             mentions a widget once among lots of filler words and more filler",
        &[],
        &[],
    );
    assert!(recent_but_weak.modified >= relevant.modified);

    let hits = collect_hits(&conn, "widget", &[], &[], &[], 20, &SemanticOpts::off()).unwrap();
    assert_eq!(hits.len(), 2, "both memories match the query");
    assert_eq!(
        hits[0].description, "widget widget widget",
        "the stronger bm25 match must lead over the more-recent weaker match"
    );
}

#[test]
fn recall_by_file_returns_associated_memories() {
    let conn = db::open_in_memory_for_test();

    let item = seed_memory(&conn, "auth notes", "JWT details", &[], &[]);
    db::set_item_files(&conn, &item.uuid, &["src/auth.rs".to_string()]).unwrap();

    seed_memory(&conn, "unrelated notes", "something else", &[], &[]);

    let hits = collect_hits(
        &conn,
        "",
        &[],
        &[],
        &["src/auth.rs".to_string()],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].description, "auth notes");
    assert_eq!(hits[0].files, vec!["src/auth.rs"]);
}

#[test]
fn recall_by_file_prefix_returns_all_files_under_dir() {
    let conn = db::open_in_memory_for_test();

    let a = seed_memory(&conn, "auth notes", "JWT details", &[], &[]);
    db::set_item_files(&conn, &a.uuid, &["src/auth.rs".to_string()]).unwrap();

    let b = seed_memory(&conn, "model notes", "model details", &[], &[]);
    db::set_item_files(&conn, &b.uuid, &["src/model.rs".to_string()]).unwrap();

    seed_memory(&conn, "unrelated", "outside src", &[], &[]);

    let hits = collect_hits(
        &conn,
        "",
        &[],
        &[],
        &["src/".to_string()],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 2);
}

#[test]
fn recall_file_and_tag_are_anded() {
    let conn = db::open_in_memory_for_test();

    let a = seed_memory(&conn, "auth notes", "JWT details", &["auth"], &[]);
    db::set_item_files(&conn, &a.uuid, &["src/auth.rs".to_string()]).unwrap();

    let b = seed_memory(&conn, "other auth notes", "other", &["auth"], &[]);
    db::set_item_files(&conn, &b.uuid, &["src/other.rs".to_string()]).unwrap();

    let hits = collect_hits(
        &conn,
        "",
        &["auth".to_string()],
        &[],
        &["src/auth.rs".to_string()],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].description, "auth notes");
}

#[test]
fn recall_linked_tasks_are_surfaced_on_item_hit() {
    let conn = db::open_in_memory_for_test();

    let mut task = Task::new("fix auth".to_string(), "Sara".to_string());
    task.status = Status::Completed;
    db::insert_task(&conn, &mut task).unwrap();

    let item = seed_memory(&conn, "auth memory", "auth body", &["auth"], &[]);
    db::set_item_task_links(&conn, &item.uuid, &[(task.uuid, "explicit")]).unwrap();

    let hits = collect_hits(
        &conn,
        "",
        &["auth".to_string()],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].linked_tasks.len(), 1);
    assert_eq!(hits[0].linked_tasks[0].0.description, "fix auth");
    assert_eq!(hits[0].linked_tasks[0].1, "explicit");
}

#[test]
fn recall_surfaces_recurring_pattern_with_guide() {
    let conn = db::open_in_memory_for_test();

    // A canonical pattern with two prior applications = a recurring pattern.
    let canonical = seed_memory(
        &conn,
        "CANONICAL NSubstitute dependabot restore fault",
        "pin NSubstitute back to 5.3.0 in every test csproj and revert TestHelper",
        &["nsubstitute"],
        &[],
    );
    let app_a = seed_memory(
        &conn,
        "repo-a PR restore fault",
        "applied the NSubstitute pin to repo-a",
        &["nsubstitute"],
        &[],
    );
    let app_b = seed_memory(
        &conn,
        "repo-b PR restore fault",
        "applied the NSubstitute pin to repo-b",
        &["nsubstitute"],
        &[],
    );
    for child in [&app_a, &app_b] {
        db::insert_memory_link(
            &conn,
            &child.uuid.to_string(),
            &canonical.uuid.to_string(),
            "derived_from",
            1.0,
        )
        .unwrap();
    }

    let v = recall_value(
        &conn,
        &cfg(),
        "",
        &["nsubstitute".to_string()],
        &[],
        &[],
        20,
        false,
    )
    .unwrap();
    let patterns = v["patterns"].as_array().expect("patterns array present");
    assert_eq!(patterns.len(), 1, "exactly one recurring pattern expected");

    let p = &patterns[0];
    let canon_label = format!("m{}", canonical.display_id.unwrap_or(0));
    assert_eq!(p["canonical"], serde_json::json!(canon_label));
    assert_eq!(p["occurrences"], serde_json::json!(2));
    // Full canonical recipe is surfaced as `text`, not a truncated preview.
    assert!(
        p["text"]
            .as_str()
            .unwrap()
            .contains("pin NSubstitute back to 5.3.0"),
        "pattern must carry the canonical's full recipe body"
    );
    // Instances name both prior applications.
    let instances: Vec<String> = p["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert_eq!(instances.len(), 2);
    // Guide tells the agent to build a task from the pattern.
    let guide = p["guide"].as_str().unwrap();
    assert!(guide.contains("add") && guide.contains("derived_from"));
}

#[test]
fn recall_reports_no_pattern_for_a_lone_memory() {
    let conn = db::open_in_memory_for_test();
    seed_memory(
        &conn,
        "one-off finding",
        "a single unrelated memory",
        &["solo"],
        &[],
    );
    let v = recall_value(
        &conn,
        &cfg(),
        "",
        &["solo".to_string()],
        &[],
        &[],
        20,
        false,
    )
    .unwrap();
    assert!(
        v["patterns"].as_array().unwrap().is_empty(),
        "a memory with no derived applications is not a recurring pattern"
    );
}

#[test]
fn recall_surfaces_canonical_and_derived_memory_distinction() {
    let conn = db::open_in_memory_for_test();

    // Canonical: the pattern memory.
    let canonical = seed_memory(
        &conn,
        "CodeQL config pattern",
        "extract to codeql-config.yml and use query-filters",
        &["codeql"],
        &[],
    );
    // Derived: per-repo application memories.
    let derived_a = seed_memory(
        &conn,
        "CodeQL config applied to repo-a",
        "applied codeql pattern to repo-a",
        &["codeql"],
        &[],
    );
    let derived_b = seed_memory(
        &conn,
        "CodeQL config applied to repo-b",
        "applied codeql pattern to repo-b",
        &["codeql"],
        &[],
    );

    // Link derived memories to canonical.
    db::insert_memory_link(
        &conn,
        &derived_a.uuid.to_string(),
        &canonical.uuid.to_string(),
        "derived_from",
        1.0,
    )
    .unwrap();
    db::insert_memory_link(
        &conn,
        &derived_b.uuid.to_string(),
        &canonical.uuid.to_string(),
        "derived_from",
        1.0,
    )
    .unwrap();

    // Distinction fields are computed per-memory in item_hit (independent of
    // the family collapse that collect_hits applies below).
    let canonical_hit = item_hit(&conn, canonical.clone(), true);
    let derived_a_hit = item_hit(&conn, derived_a.clone(), true);
    let derived_b_hit = item_hit(&conn, derived_b.clone(), true);

    // Canonical: has 2 derived children (by label), no derived_from_labels.
    assert_eq!(
        canonical_hit.derived_children.len(),
        2,
        "canonical must report 2 derived memories"
    );
    assert!(
        canonical_hit.derived_from_labels.is_empty(),
        "canonical must not be derived from anything"
    );
    // The canonical's child labels must be exactly its two derived memories.
    assert!(
        canonical_hit
            .derived_children
            .contains(&derived_a_hit.label)
            && canonical_hit
                .derived_children
                .contains(&derived_b_hit.label),
        "canonical must list both derived child labels, got {:?}",
        canonical_hit.derived_children
    );

    // Derived: has no children, derived_from_labels pointing to canonical.
    assert!(derived_a_hit.derived_children.is_empty());
    assert_eq!(derived_a_hit.derived_from_labels.len(), 1);
    assert!(derived_b_hit.derived_children.is_empty());
    assert_eq!(derived_b_hit.derived_from_labels.len(), 1);

    // Both derived memories point to the same canonical label.
    assert_eq!(
        derived_a_hit.derived_from_labels[0],
        derived_b_hit.derived_from_labels[0]
    );

    // collect_hits COLLAPSES the family: the canonical + its 2 children fold
    // into a single representative (the canonical, which sorts first via its
    // strength bonus), tagged with the family size.
    let hits = collect_hits(
        &conn,
        "",
        &["codeql".to_string()],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(
        hits.len(),
        1,
        "the canonical family collapses to one representative, got: {:?}",
        hits.iter().map(|h| &h.description).collect::<Vec<_>>()
    );
    assert_eq!(
        hits[0].description, "CodeQL config pattern",
        "the canonical is the surviving representative"
    );
    let cluster = hits[0]
        .cluster
        .as_ref()
        .expect("the representative carries cluster metadata");
    assert_eq!(cluster.canonical_label, canonical_hit.label);
    assert_eq!(cluster.size, 3, "family size = canonical + 2 children");
    assert_eq!(cluster.collapsed_here, 2, "2 siblings folded out");
}

#[test]
fn recall_collapses_family_to_canonical_even_when_canonical_ranks_lower() {
    let conn = db::open_in_memory_for_test();

    // A canonical with three per-application derived children — the shape of
    // a recurring fault (e.g. the dependabot/NSubstitute family) that would
    // otherwise flood recall with near-duplicates.
    let canonical = seed_memory(&conn, "canonical fix", "the pattern", &["dep"], &[]);
    let mut children = vec![];
    for i in 0..3 {
        let c = seed_memory(
            &conn,
            &format!("applied fix #{i}"),
            &format!("per-PR application {i}"),
            &["dep"],
            &[],
        );
        db::insert_memory_link(
            &conn,
            &c.uuid.to_string(),
            &canonical.uuid.to_string(),
            "derived_from",
            1.0,
        )
        .unwrap();
        children.push(c);
    }

    let hits = collect_hits(
        &conn,
        "",
        &["dep".to_string()],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();

    // Four memories in the corpus, one representative in the result.
    assert_eq!(
        hits.len(),
        1,
        "a 4-member family returns a single representative, got: {:?}",
        hits.iter().map(|h| &h.description).collect::<Vec<_>>()
    );
    let rep = &hits[0];
    assert_eq!(
        rep.description, "canonical fix",
        "the canonical is promoted to representative"
    );
    let cluster = rep.cluster.as_ref().expect("cluster metadata present");
    assert_eq!(cluster.size, 4, "canonical + 3 children");
    assert_eq!(cluster.collapsed_here, 3);
}

#[test]
fn recall_leaves_standalone_memories_uncollapsed() {
    let conn = db::open_in_memory_for_test();

    // Two unrelated memories with no derived_from linkage — neither should be
    // collapsed or tagged with cluster metadata.
    seed_memory(&conn, "note one", "body one", &["misc"], &[]);
    seed_memory(&conn, "note two", "body two", &["misc"], &[]);

    let hits = collect_hits(
        &conn,
        "",
        &["misc".to_string()],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert_eq!(hits.len(), 2, "unrelated memories are not collapsed");
    assert!(
        hits.iter().all(|h| h.cluster.is_none()),
        "standalone memories carry no cluster metadata"
    );
}

#[test]
fn recall_by_project_keeps_local_child_separate_from_foreign_canonical() {
    let conn = db::open_in_memory_for_test();

    // Canonical lives only under "sara-repo".
    let canonical = seed_memory(
        &conn,
        "CodeQL config pattern",
        "extract to codeql-config.yml",
        &[],
        &["sara-repo"],
    );
    // Derived application lives under a completely different project.
    let derived = seed_memory(
        &conn,
        "applied CodeQL config to other-repo",
        "applied the pattern here",
        &[],
        &["other-repo"],
    );
    db::insert_memory_link(
        &conn,
        &derived.uuid.to_string(),
        &canonical.uuid.to_string(),
        "derived_from",
        1.0,
    )
    .unwrap();

    // A --project other-repo recall must NOT fold the local child under the
    // foreign-project canonical: collapsing is now project-scoped, so a
    // canonical from another repo can never stand in for (and hide) a memory
    // that actually belongs to the queried project.
    let hits = collect_hits(
        &conn,
        "",
        &[],
        &["other-repo".to_string()],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert!(
        hits.iter()
            .any(|h| h.description == "applied CodeQL config to other-repo"),
        "the local child stays visible, got: {:?}",
        hits.iter().map(|h| &h.description).collect::<Vec<_>>()
    );
    // The foreign canonical and the local child do not collapse together —
    // different project signatures keep them as distinct representatives.
    assert!(
        hits.len() >= 2,
        "local child and foreign canonical are not merged, got: {:?}",
        hits.iter().map(|h| &h.description).collect::<Vec<_>>()
    );

    // A project with no derived children anywhere must not pull the
    // canonical in.
    let unrelated_hits = collect_hits(
        &conn,
        "",
        &[],
        &["unrelated-repo".to_string()],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap();
    assert!(unrelated_hits.is_empty());
}
