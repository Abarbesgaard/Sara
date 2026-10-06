use super::*;
use crate::infrastructure::config::Config;
use crate::infrastructure::model::{Status, Task};

use crate::test_support::cfg;

fn seed_memory(
    conn: &Connection,
    title: &str,
    body: &str,
    tags: &[&str],
    projects: &[&str],
) -> Item {
    let item = crate::test_support::seed_memory(conn, title, body, tags);
    db::set_item_projects(
        conn,
        &item.uuid,
        &projects.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
    )
    .unwrap();
    item
}

fn cfg_semantic() -> Config {
    let mut c = Config::default();
    c.recall.semantic = true;
    c
}

#[test]
fn recall_semantic_surfaces_paraphrase() {
    let conn = db::open_in_memory_for_test();
    let m = seed_memory(
        &conn,
        "CI restore step failed",
        "a dependabot bump broke the build; pin the lockfile version to fix it",
        &[],
        &[],
    );
    crate::infrastructure::memory::embedding::index_memory(&conn, &m);

    let query = "automated dependency update wrecked the pipeline";

    let lexical = collect_hits(&conn, query, &[], &[], &[], 20, &SemanticOpts::off()).unwrap();
    assert!(
        lexical.is_empty(),
        "lexical recall should NOT surface the paraphrase, got {} hits",
        lexical.len()
    );

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
    let conn = db::open_in_memory_for_test();

    let untagged = seed_memory(
        &conn,
        "CI restore step failed",
        "a dependabot bump broke the build; pin the lockfile version to fix it",
        &[],
        &[],
    );
    crate::infrastructure::memory::embedding::index_memory(&conn, &untagged);

    let tagged = seed_memory(
        &conn,
        "unrelated note",
        "remember to water the office plants on fridays",
        &["ci"],
        &[],
    );
    crate::infrastructure::memory::embedding::index_memory(&conn, &tagged);

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
    let conn = db::open_in_memory_for_test();

    let query = "lockfile pin fixes the broken dependabot build";

    let outside = seed_memory(
        &conn,
        "outside filter",
        "lockfile pin fixes the broken dependabot build",
        &[],
        &[],
    );
    crate::infrastructure::memory::embedding::index_memory(&conn, &outside);

    let inside = seed_memory(&conn, "inside filter", "lockfile pin broke", &["keep"], &[]);
    crate::infrastructure::memory::embedding::index_memory(&conn, &inside);

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

    let v = recall_value(&conn, &cfg(), "uniquewidget", &[], &[], &[], 20, false).unwrap();
    let hits = v["keyword"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "the memory should surface");
    assert_eq!(
        hits[0]["text"].as_str().unwrap(),
        long_body,
        "recall JSON must carry the complete memory body, not a 160-char preview"
    );

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
    assert!(
        hits[0]["text"].as_str().is_some_and(|t| t.len() > 160),
        "top hit must carry its full untruncated body"
    );
    for h in &hits[1..] {
        assert!(h["text"].is_null(), "non-top hits carry no full body: {h}");
        assert!(
            h["preview"].as_str().is_some(),
            "guide entries still carry a preview and label"
        );
        assert!(h["label"].as_str().is_some());
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
    crate::test_support::link(&conn, seed.uuid, "similar_to", neighbour.uuid);
    let label = format!("m{}", seed.display_id.unwrap());

    let v = recall_value(&conn, &cfg(), &label, &[], &[], &[], 10, false).unwrap();
    assert_eq!(v["deep"], true, "label recall is a deep drill-in");
    assert_eq!(v["spread"], "label");
    let hits = v["keyword"].as_array().unwrap();
    assert_eq!(hits.len(), 1, "exactly the resolved memory");
    assert!(
        hits[0]["text"].as_str().is_some_and(|t| t.len() > 160),
        "the drilled-into memory comes back in full"
    );
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
    let conn = db::open_in_memory_for_test();
    let m = seed_memory(
        &conn,
        "CI restore step failed",
        "a dependabot bump broke the build; pin the lockfile version",
        &[],
        &[],
    );
    crate::infrastructure::memory::embedding::index_memory(&conn, &m);

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
    crate::test_support::link(&conn, seed.uuid, "similar_to", neighbour.uuid);

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
    let outsider = seed_memory(
        &conn,
        "outsider",
        "unrelated mutex trick",
        &[],
        &["web-app"],
    );
    crate::test_support::link(&conn, a.uuid, "similar_to", outsider.uuid);
    crate::test_support::link(&conn, b.uuid, "similar_to", outsider.uuid);

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
    crate::test_support::link(&conn, a.uuid, "similar_to", outsider.uuid);

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
    crate::test_support::link(&conn, seed.uuid, "similar_to", neighbour.uuid);

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
    crate::test_support::link(&conn, seed.uuid, "similar_to", neighbour.uuid);

    assert_eq!(db::item_strength(&conn, &neighbour), 1.0);

    let v = recall_value(&conn, &cfg(), "pgbouncer", &[], &[], &[], 20, true).unwrap();
    assert!(
        v["associative"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["preview"].as_str() == Some("raise pool_size for burst traffic")),
        "the neighbour must surface via spreading activation for this test to be meaningful"
    );

    assert_eq!(
        db::item_strength(&conn, &neighbour),
        1.0,
        "a spread-only surfacing must not reinforce strength like a deliberate recall"
    );
}

#[test]
fn spreading_related_is_empty_without_memory_seeds() {
    let conn = db::open_in_memory_for_test();
    let related = spreading_related(&conn, &[]).unwrap();
    assert!(related.is_empty());
}

#[test]
fn associative_output_is_capped_and_normalized() {
    let conn = db::open_in_memory_for_test();
    let hub = seed_memory(&conn, "hub topic", "central memory", &["hub"], &["p"]);
    for i in 0..12 {
        let n = seed_memory(&conn, &format!("n{i}"), &format!("body {i}"), &[], &["p"]);
        crate::test_support::link(&conn, hub.uuid, "similar_to", n.uuid);
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
    let conn = db::open_in_memory_for_test();
    seed_memory(
        &conn,
        "prune stale entries",
        "pruning removes memories that decay over time",
        &[],
        &["engine"],
    );

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

    let (confidence, caveat) = match_confidence("pruning decay archive garbage", &[], &hits);
    assert_eq!(confidence, "low");
    assert!(caveat.contains("Loose match"));
}

#[test]
fn token_or_fallback_does_not_fire_when_stricter_tiers_match() {
    let conn = db::open_in_memory_for_test();
    seed_memory(
        &conn,
        "kafka consumer lag",
        "monitor consumer group lag with burrow",
        &[],
        &["platform"],
    );
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
    let relevant = seed_memory(&conn, "widget widget widget", "widget", &[], &[]);
    std::thread::sleep(std::time::Duration::from_millis(20));
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
        crate::test_support::link(&conn, child.uuid, "derived_from", canonical.uuid);
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
    assert!(
        p["text"]
            .as_str()
            .unwrap()
            .contains("pin NSubstitute back to 5.3.0"),
        "pattern must carry the canonical's full recipe body"
    );
    let instances: Vec<String> = p["instances"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap().to_string())
        .collect();
    assert_eq!(instances.len(), 2);
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

    let canonical = seed_memory(
        &conn,
        "CodeQL config pattern",
        "extract to codeql-config.yml and use query-filters",
        &["codeql"],
        &[],
    );
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

    crate::test_support::link(&conn, derived_a.uuid, "derived_from", canonical.uuid);
    crate::test_support::link(&conn, derived_b.uuid, "derived_from", canonical.uuid);

    let canonical_hit = item_hit(&conn, canonical.clone(), true);
    let derived_a_hit = item_hit(&conn, derived_a.clone(), true);
    let derived_b_hit = item_hit(&conn, derived_b.clone(), true);

    assert_eq!(
        canonical_hit.derived_children.len(),
        2,
        "canonical must report 2 derived memories"
    );
    assert!(
        canonical_hit.derived_from_labels.is_empty(),
        "canonical must not be derived from anything"
    );
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

    assert!(derived_a_hit.derived_children.is_empty());
    assert_eq!(derived_a_hit.derived_from_labels.len(), 1);
    assert!(derived_b_hit.derived_children.is_empty());
    assert_eq!(derived_b_hit.derived_from_labels.len(), 1);

    assert_eq!(
        derived_a_hit.derived_from_labels[0],
        derived_b_hit.derived_from_labels[0]
    );

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
        crate::test_support::link(&conn, c.uuid, "derived_from", canonical.uuid);
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

    let canonical = seed_memory(
        &conn,
        "CodeQL config pattern",
        "extract to codeql-config.yml",
        &[],
        &["sara-repo"],
    );
    let derived = seed_memory(
        &conn,
        "applied CodeQL config to other-repo",
        "applied the pattern here",
        &[],
        &["other-repo"],
    );
    crate::test_support::link(&conn, derived.uuid, "derived_from", canonical.uuid);

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
    assert!(
        hits.len() >= 2,
        "local child and foreign canonical are not merged, got: {:?}",
        hits.iter().map(|h| &h.description).collect::<Vec<_>>()
    );

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

fn seed_family(conn: &Connection, children: usize) -> (Item, Vec<Item>) {
    let canonical = seed_memory(conn, "canonical", "the pattern", &["fam"], &[]);
    let kids = (0..children)
        .map(|i| {
            let c = seed_memory(conn, &format!("child {i}"), "applied", &["fam"], &[]);
            crate::test_support::link(conn, c.uuid, "derived_from", canonical.uuid);
            c
        })
        .collect();
    (canonical, kids)
}

fn hits_in_order(conn: &Connection, items: &[&Item]) -> Vec<Hit> {
    items
        .iter()
        .map(|i| {
            item_hit(
                conn,
                db::get_item_by_uuid(conn, &i.uuid.to_string()).unwrap(),
                false,
            )
        })
        .collect()
}

#[test]
fn cluster_nearest_names_child_displaced_by_canonical() {
    let conn = db::open_in_memory_for_test();
    let (canonical, kids) = seed_family(&conn, 2);

    let hits = hits_in_order(&conn, &[&kids[0], &kids[1], &canonical]);
    let top_child = hits[0].label.clone();
    let out = collapse_clusters(&conn, hits);

    assert_eq!(out.len(), 1);
    assert_eq!(
        out[0].description, "canonical",
        "canonical stays representative"
    );
    let c = out[0].cluster.as_ref().unwrap();
    assert_eq!(c.collapsed_here, 2);
    assert_eq!(
        c.nearest.as_deref(),
        Some(top_child.as_str()),
        "the displaced top-ranked child is named, not the lower sibling"
    );
}

#[test]
fn cluster_nearest_names_top_collapsed_sibling_when_rep_not_displaced() {
    let conn = db::open_in_memory_for_test();
    let (canonical, kids) = seed_family(&conn, 3);

    let hits = hits_in_order(&conn, &[&canonical, &kids[2], &kids[0], &kids[1]]);
    let first_sibling = hits[1].label.clone();
    let rep_label = hits[0].label.clone();
    let out = collapse_clusters(&conn, hits);

    let c = out[0].cluster.as_ref().unwrap();
    assert_eq!(c.collapsed_here, 3);
    assert_eq!(c.nearest.as_deref(), Some(first_sibling.as_str()));
    assert_ne!(c.nearest.as_deref(), Some(rep_label.as_str()));
}

#[test]
fn cluster_nearest_is_none_when_nothing_collapsed() {
    let conn = db::open_in_memory_for_test();
    let (canonical, _kids) = seed_family(&conn, 2);

    let out = collapse_clusters(&conn, hits_in_order(&conn, &[&canonical]));

    let c = out[0].cluster.as_ref().expect("family of 3 still tagged");
    assert_eq!(c.collapsed_here, 0);
    assert_eq!(c.nearest, None);
}

#[test]
fn cluster_nearest_emitted_in_json() {
    let conn = db::open_in_memory_for_test();
    let (canonical, kids) = seed_family(&conn, 1);

    let hits = hits_in_order(&conn, &[&kids[0], &canonical]);
    let child = hits[0].label.clone();
    let out = collapse_clusters(&conn, hits);
    let json = keyword_json(&out);

    assert_eq!(json[0]["cluster"]["nearest"], serde_json::json!(child));
}

#[test]
fn cluster_nearest_in_guide_json_only_when_present() {
    let conn = db::open_in_memory_for_test();
    let lead = seed_memory(&conn, "standalone lead", "unrelated", &["fam"], &[]);
    let (canonical, kids) = seed_family(&conn, 1);

    let hits = hits_in_order(&conn, &[&lead, &kids[0], &canonical]);
    let child = hits[1].label.clone();
    let json = keyword_json(&collapse_clusters(&conn, hits));
    assert_eq!(json[1]["cluster"]["nearest"], serde_json::json!(child));

    let json = keyword_json(&collapse_clusters(
        &conn,
        hits_in_order(&conn, &[&lead, &canonical]),
    ));
    assert!(json[1]["cluster"].get("nearest").is_none());
}

fn anchored_memory(conn: &Connection, dir: &std::path::Path, tag: &str) -> (Item, String) {
    let path = dir.join(format!("{tag}.rs"));
    let body: String = (0..40)
        .map(|i| format!("let step_{i} = run({i});\n"))
        .collect();
    std::fs::write(&path, body).unwrap();
    let p = path.to_str().unwrap().to_string();
    let m = seed_memory(
        conn,
        &format!("{tag} guidance"),
        "how it works",
        &[tag],
        &[],
    );
    db::set_item_files(conn, &m.uuid, std::slice::from_ref(&p)).unwrap();
    (m, p)
}

fn rewrite(path: &str) {
    let body: String = (0..40)
        .map(|i| format!("fn rewritten_{i}() {{}}\n"))
        .collect();
    std::fs::write(path, body).unwrap();
}

fn hits_for(conn: &Connection, tag: &str) -> Vec<Hit> {
    collect_hits(
        conn,
        "",
        &[tag.to_string()],
        &[],
        &[],
        20,
        &SemanticOpts::off(),
    )
    .unwrap()
}

#[test]
fn stale_not_flagged_when_anchor_unchanged() {
    let conn = db::open_in_memory_for_test();
    let dir = tempfile::tempdir().unwrap();
    anchored_memory(&conn, dir.path(), "calm");

    let hits = hits_for(&conn, "calm");
    assert!(hits[0].stale.is_empty());
    assert_eq!(keyword_json(&hits)[0]["stale"], serde_json::json!([]));
}

#[test]
fn stale_flags_drifted_anchor_in_json() {
    let conn = db::open_in_memory_for_test();
    let dir = tempfile::tempdir().unwrap();
    let (_m, p) = anchored_memory(&conn, dir.path(), "drift");
    rewrite(&p);

    let hits = hits_for(&conn, "drift");
    assert_eq!(hits[0].stale.len(), 1);
    let stale = &keyword_json(&hits)[0]["stale"][0];
    assert_eq!(stale["file"], serde_json::json!(p));
    assert_eq!(stale["reason"], "drifted");
    assert_eq!(stale["drift"], serde_json::json!(1.0));
    assert!(stale_text(&hits[0]).contains("(100% changed)"));
}

#[test]
fn stale_flags_missing_anchor_on_label_and_recent_paths() {
    let conn = db::open_in_memory_for_test();
    let dir = tempfile::tempdir().unwrap();
    let (m, p) = anchored_memory(&conn, dir.path(), "gone");
    std::fs::remove_file(&p).unwrap();

    let label = format!("m{}", m.display_id.unwrap());
    let v = recall_value(&conn, &cfg(), &label, &[], &[], &[], 5, false).unwrap();
    assert_eq!(v["keyword"][0]["stale"][0]["reason"], "missing");

    let v = recall_value(&conn, &cfg(), "", &[], &[], &[], 5, false).unwrap();
    assert_eq!(v["keyword"][0]["stale"][0]["reason"], "missing");
}

#[test]
fn stale_cleared_by_relearn() {
    let conn = db::open_in_memory_for_test();
    let dir = tempfile::tempdir().unwrap();
    let (m, p) = anchored_memory(&conn, dir.path(), "fixed");
    rewrite(&p);
    assert_eq!(hits_for(&conn, "fixed")[0].stale.len(), 1);

    let label = format!("m{}", m.display_id.unwrap());
    crate::commands::relearn::relearn_value(
        &conn,
        &label,
        Some("re-validated against the rewrite"),
        &[],
        &[],
        false,
    )
    .unwrap();

    assert!(hits_for(&conn, "fixed")[0].stale.is_empty());
}

#[test]
fn stale_ignores_legacy_anchor_without_fingerprint() {
    let conn = db::open_in_memory_for_test();
    let dir = tempfile::tempdir().unwrap();
    let (m, p) = anchored_memory(&conn, dir.path(), "legacy");
    conn.execute(
        "UPDATE item_files SET fingerprint = NULL WHERE item_uuid = ?1",
        [m.uuid.to_string()],
    )
    .unwrap();
    rewrite(&p);

    assert!(hits_for(&conn, "legacy")[0].stale.is_empty());
}

fn ledger_task(conn: &Connection) -> Task {
    let mut task = Task::new("use the cache notes".to_string(), "Sara".to_string());
    db::insert_task(conn, &mut task).unwrap();
    task
}

#[test]
fn recall_task_records_direct_hits_as_recalled_and_spread_as_surfaced() {
    let conn = db::open_in_memory_for_test();
    let seed = seed_memory(
        &conn,
        "redis eviction policy",
        "set maxmemory-policy allkeys-lru",
        &[],
        &[],
    );
    let neighbour = seed_memory(
        &conn,
        "cache stampede guard",
        "add a mutex around cold-cache fills",
        &[],
        &[],
    );
    crate::test_support::link(&conn, seed.uuid, "similar_to", neighbour.uuid);
    let task = ledger_task(&conn);

    let use_task = use_task(&conn, Some(&task.uuid.to_string()[..8])).unwrap();
    db::with_use_attribution(use_task, || {
        recall_value(&conn, &cfg(), "maxmemory-policy", &[], &[], &[], 20, true).unwrap()
    });

    let uses = db::memory_uses_for_task(&conn, &task.uuid).unwrap();
    let kinds: Vec<_> = uses.iter().map(|u| (u.item_uuid, u.kind)).collect();
    assert!(
        kinds.contains(&(seed.uuid, db::MemoryUseKind::Recalled)),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&(neighbour.uuid, db::MemoryUseKind::Surfaced)),
        "{kinds:?}"
    );
    assert!(
        !kinds.contains(&(neighbour.uuid, db::MemoryUseKind::Recalled)),
        "an associative hit is never a recall"
    );
}

#[test]
fn recall_task_absent_records_no_use_and_the_scope_does_not_leak() {
    let conn = db::open_in_memory_for_test();
    let memory = seed_memory(
        &conn,
        "redis eviction policy",
        "set maxmemory-policy allkeys-lru",
        &[],
        &[],
    );
    let task = ledger_task(&conn);

    db::with_use_attribution(Some(task.uuid), || {});
    recall_value(&conn, &cfg(), "maxmemory-policy", &[], &[], &[], 20, false).unwrap();

    assert!(
        db::memory_uses_for_item(&conn, &memory.uuid)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn recall_task_unknown_id_is_an_error() {
    let conn = db::open_in_memory_for_test();
    assert!(use_task(&conn, Some("zzzzzzzz")).is_err());
    assert_eq!(use_task(&conn, None).unwrap(), None);
}
