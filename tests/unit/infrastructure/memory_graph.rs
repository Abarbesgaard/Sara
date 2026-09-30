use super::*;

fn seed(conn: &Connection, tags: &[&str]) -> Uuid {
    crate::test_support::seed_memory(conn, "t", "body", tags).uuid
}

#[test]
fn shared_tag_creates_a_weighted_edge() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["auth"]);
    let b = seed(&conn, &["auth"]);
    let c = seed(&conn, &["billing"]);

    let g = MemoryGraph::build(&conn).unwrap();
    assert_eq!(g.nodes.len(), 3);
    let idf = (3.0_f64 / 2.0).ln() / 3.0_f64.ln();
    let expected = W_SHARED_TAG * idf;
    assert!((g.edge_weight(&a, &b).unwrap() - expected).abs() < 1e-9);
    assert!(expected > 0.0 && expected < W_SHARED_TAG);
    assert_eq!(g.edge_weight(&a, &c), None);
}

fn assert_matches_naive_reference(tags_by_node: &[Vec<String>], expect_inverted: bool) {
    let conn = db::open_in_memory_for_test();
    let n = tags_by_node.len();
    let uuids: Vec<Uuid> = tags_by_node
        .iter()
        .map(|tags| {
            let refs: Vec<&str> = tags.iter().map(String::as_str).collect();
            seed(&conn, &refs)
        })
        .collect();

    let mut df: HashMap<&str, usize> = HashMap::new();
    for set in tags_by_node {
        for t in set {
            *df.entry(t.as_str()).or_insert(0) += 1;
        }
    }

    let inverted: u128 = df
        .values()
        .map(|&d| {
            if d < 2 || d >= n {
                0
            } else {
                (d as u128) * (d as u128 - 1) / 2
            }
        })
        .sum();
    let dense = (n as u128) * (n as u128 - 1) / 2;
    assert_eq!(
        inverted.saturating_mul(INVERT_OVERHEAD) < dense,
        expect_inverted,
        "fixture selected the wrong strategy (inverted={inverted}, dense={dense})"
    );

    let g = MemoryGraph::build(&conn).unwrap();
    assert_eq!(g.nodes.len(), n);

    let idf = |d: usize| -> f64 {
        if n < 2 || d == 0 || d >= n {
            return 0.0;
        }
        (n as f64 / d as f64).ln() / (n as f64).ln()
    };
    let mut expected: HashMap<(usize, usize), f64> = HashMap::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let mut w = 0.0;
            for t in &tags_by_node[i] {
                if tags_by_node[j].contains(t) {
                    w += W_SHARED_TAG * idf(df[t.as_str()]);
                }
            }
            if w > 0.0 {
                expected.insert((i, j), w.min(MAX_EDGE));
            }
        }
    }

    assert!(
        expected.len() > 50,
        "fixture is too sparse to be meaningful: {} edges",
        expected.len()
    );
    assert_eq!(
        g.edge_count(),
        expected.len(),
        "edge count diverged from the naive reference"
    );
    for ((i, j), want) in expected {
        let got = g
            .edge_weight(&uuids[i], &uuids[j])
            .unwrap_or_else(|| panic!("edge {i}-{j} missing from the optimised graph"));
        assert!(
            (got - want).abs() < 1e-12,
            "edge {i}-{j}: got {got}, want {want}"
        );
    }
}

fn rng() -> impl FnMut() -> u64 {
    let mut state: u64 = 0x2545_F491_4F6C_DD1D;
    move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    }
}

#[test]
fn sparse_anchors_take_the_inverted_path_and_match_the_naive_reference() {
    const N: usize = 120;
    let mut next = rng();
    let tags_by_node: Vec<Vec<String>> = (0..N)
        .map(|_| {
            let mut tags: Vec<String> = vec!["memory".into()];
            for _ in 0..(1 + next() % 3) {
                tags.push(format!("topic-{}", next() % 40));
            }
            dedup(tags)
        })
        .collect();
    assert_matches_naive_reference(&tags_by_node, true);
}

#[test]
fn a_near_ubiquitous_anchor_takes_the_dense_path_and_match_the_naive_reference() {
    const N: usize = 120;
    let mut next = rng();
    let tags_by_node: Vec<Vec<String>> = (0..N)
        .map(|i| {
            let mut tags: Vec<String> = vec!["memory".into()];
            if i % 3 != 0 {
                tags.push("common".into());
            }
            for _ in 0..(next() % 3) {
                tags.push(format!("topic-{}", next() % 12));
            }
            dedup(tags)
        })
        .collect();
    assert_matches_naive_reference(&tags_by_node, false);
}

#[test]
fn duplicate_anchor_within_a_memory_does_not_inflate_edge() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["auth", "auth"]);
    let b = seed(&conn, &["auth"]);
    let _c = seed(&conn, &["billing"]);

    let g = MemoryGraph::build(&conn).unwrap();
    let idf = (3.0_f64 / 2.0).ln() / 3.0_f64.ln();
    let expected = W_SHARED_TAG * idf;
    assert!((g.edge_weight(&a, &b).unwrap() - expected).abs() < 1e-9);
}

#[test]
fn explicit_link_and_shared_anchor_sum() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["auth"]);
    let b = seed(&conn, &["auth"]);
    crate::test_support::link(&conn, a, "similar_to", b);

    let g = MemoryGraph::build(&conn).unwrap();
    assert!((g.edge_weight(&a, &b).unwrap() - 0.7).abs() < 1e-9);
}

#[test]
fn activation_spreads_to_two_hop_neighbour_and_decays() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["x"]);
    let b = seed(&conn, &["x", "y"]);
    let c = seed(&conn, &["y"]);

    let g = MemoryGraph::build(&conn).unwrap();
    let ranked = g.spread_activation(&[a], 2, 0.6, 1e-6);
    let act: HashMap<Uuid, f64> = ranked.into_iter().collect();

    assert!(act[&a] > act[&b]);
    assert!(act[&b] > act[&c]);
    assert!(act[&c] > 0.0, "two-hop neighbour must be activated");
}

#[test]
fn explained_spread_reconstructs_the_synaptic_path() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["x"]);
    let b = seed(&conn, &["x", "y"]);
    let c = seed(&conn, &["y"]);

    let g = MemoryGraph::build(&conn).unwrap();
    let explained = g.spread_activation_explained(&[a], 2, 0.6, 1e-6);

    let seed_label = g.nodes[g.index[&a]].label.clone();
    let mid_label = g.nodes[g.index[&b]].label.clone();
    let far_label = g.nodes[g.index[&c]].label.clone();

    let seed_act = explained.iter().find(|e| e.uuid == a).unwrap();
    assert_eq!(seed_act.path, vec![seed_label.clone()]);

    let far = explained.iter().find(|e| e.uuid == c).unwrap();
    assert_eq!(far.path, vec![seed_label, mid_label, far_label]);
}

#[test]
fn unconnected_memory_is_not_activated() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["x"]);
    let lone = seed(&conn, &["unrelated"]);
    let g = MemoryGraph::build(&conn).unwrap();
    let reached: Vec<Uuid> = g
        .spread_activation(&[a], 3, 0.6, 1e-6)
        .into_iter()
        .map(|(u, _)| u)
        .collect();
    assert!(reached.contains(&a));
    assert!(!reached.contains(&lone));
}

#[test]
fn rare_shared_anchor_binds_tighter_than_a_ubiquitous_one() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["common", "rare"]);
    let b = seed(&conn, &["rare"]);
    let mut hubs = vec![];
    for _ in 0..8 {
        hubs.push(seed(&conn, &["common"]));
    }

    let g = MemoryGraph::build(&conn).unwrap();
    let rare_edge = g.edge_weight(&a, &b).unwrap();
    let hub_edge = g.edge_weight(&a, &hubs[0]).unwrap();
    assert!(
        rare_edge > hub_edge,
        "a rare shared anchor must bind tighter than a ubiquitous one (idf): rare={rare_edge} hub={hub_edge}"
    );
}

#[test]
fn bulk_recall_bucket_is_ignored_as_noise() {
    let t0 = Utc::now();
    let ids: Vec<Uuid> = (0..6).map(|_| Uuid::new_v4()).collect();
    let events: Vec<_> = ids.iter().map(|u| (*u, t0)).collect();

    let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
    assert!(
        pairs.is_empty(),
        "a bucket over the max size must yield no co-firing pairs, got {}",
        pairs.len()
    );
}

#[test]
fn coactivation_pairs_group_within_bucket() {
    let t0 = DateTime::from_timestamp_millis(1_700_000_001_900).unwrap();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let c = Uuid::new_v4();
    let events = vec![
        (a, t0),
        (b, t0 + Duration::milliseconds(100)),
        (c, t0 + Duration::seconds(60)),
    ];
    let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
    assert_eq!(pairs.len(), 1);
    let (x, y, count) = pairs[0];
    assert_eq!(count, 1);
    let got = if x < y { (x, y) } else { (y, x) };
    let want = if a < b { (a, b) } else { (b, a) };
    assert_eq!(got, want);
}

#[test]
fn coactivation_is_translation_invariant() {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    for offset_ms in 0..2000 {
        let t0 = DateTime::from_timestamp_millis(1_700_000_000_000 + offset_ms).unwrap();
        let events = vec![(a, t0), (b, t0 + Duration::milliseconds(100))];
        let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
        assert_eq!(
            pairs.len(),
            1,
            "lost the co-firing at start offset {offset_ms}ms"
        );
    }
}

#[test]
fn coactivation_survives_an_unrelated_preceding_recall() {
    let x = Uuid::new_v4();
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    let t = DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
    for lead_ms in 0..2000 {
        let events = vec![
            (x, t - Duration::milliseconds(lead_ms)),
            (a, t),
            (b, t + Duration::milliseconds(100)),
        ];
        let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
        assert!(
            pairs
                .iter()
                .any(|(p, q, _)| (*p == a && *q == b) || (*p == b && *q == a)),
            "lost the a/b co-firing when an unrelated recall preceded it by {lead_ms}ms"
        );
    }
}

#[test]
fn coactivation_guard_discards_a_long_chained_burst() {
    let t = DateTime::from_timestamp_millis(1_700_000_000_000).unwrap();
    let events: Vec<(Uuid, DateTime<Utc>)> = (0..40)
        .map(|i| (Uuid::new_v4(), t + Duration::milliseconds(i * 10)))
        .collect();
    let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
    assert!(
        pairs.is_empty(),
        "a 40-memory chained burst is a bulk listing, not co-firing; got {} pairs",
        pairs.len()
    );
    let unguarded = coactivation_pairs(&events, Duration::seconds(2), 0);
    assert_eq!(unguarded.len(), 40 * 39 / 2);
}

#[test]
fn coactivation_splits_events_beyond_the_window() {
    let a = Uuid::new_v4();
    let b = Uuid::new_v4();
    for offset_ms in 0..500 {
        let t0 = DateTime::from_timestamp_millis(1_700_000_000_000 + offset_ms).unwrap();
        let events = vec![(a, t0), (b, t0 + Duration::milliseconds(2001))];
        let pairs = coactivation_pairs(&events, Duration::seconds(2), 5);
        assert!(
            pairs.is_empty(),
            "spurious co-firing at start offset {offset_ms}ms"
        );
    }
}

#[test]
fn consolidate_reinforces_a_co_activated_edge_from_recall_events() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["x"]);
    let b = seed(&conn, &["y"]);

    for _ in 0..2 {
        db::record_memory_recall(&conn, &a).unwrap();
        db::record_memory_recall(&conn, &b).unwrap();
    }

    let reinforced = consolidate(&conn, 30, Duration::seconds(2), 0.1, 5).unwrap();
    assert_eq!(reinforced, 1);

    let g = MemoryGraph::build(&conn).unwrap();
    assert!(
        g.edge_weight(&a, &b).is_some(),
        "co-activation should have wired a and b together"
    );
}

fn co_activated_weight(conn: &Connection, a: &Uuid, b: &Uuid) -> Option<f64> {
    let (a, b) = (a.to_string(), b.to_string());
    let (from, to) = if a < b { (a, b) } else { (b, a) };
    db::all_memory_links(conn)
        .unwrap()
        .into_iter()
        .find(|l| l.relation == "co_activated" && l.from_uuid == from && l.to_uuid == to)
        .map(|l| l.weight)
}

#[test]
fn consolidate_is_idempotent_over_the_same_events() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["x"]);
    let b = seed(&conn, &["y"]);
    db::record_memory_recall(&conn, &a).unwrap();
    db::record_memory_recall(&conn, &b).unwrap();

    consolidate(&conn, 30, Duration::seconds(2), 0.1, 5).unwrap();
    let first = co_activated_weight(&conn, &a, &b).expect("edge after first run");
    for _ in 0..3 {
        consolidate(&conn, 30, Duration::seconds(2), 0.1, 5).unwrap();
    }
    let again = co_activated_weight(&conn, &a, &b).expect("edge after reruns");
    assert!(
        (first - 0.1).abs() < 1e-9 && (again - first).abs() < 1e-9,
        "weight drifted across reruns: first={first}, again={again}"
    );
}

#[test]
fn consolidate_drops_synapses_whose_cofirings_left_the_window() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["x"]);
    let b = seed(&conn, &["y"]);
    db::record_memory_recall(&conn, &a).unwrap();
    db::record_memory_recall(&conn, &b).unwrap();
    consolidate(&conn, 30, Duration::seconds(2), 0.1, 5).unwrap();
    assert!(co_activated_weight(&conn, &a, &b).is_some());

    let old = (Utc::now() - Duration::days(60)).to_rfc3339();
    conn.execute("UPDATE events SET at = ?1", [old]).unwrap();
    let reinforced = consolidate(&conn, 30, Duration::seconds(2), 0.1, 5).unwrap();
    assert_eq!(reinforced, 0);
    assert_eq!(co_activated_weight(&conn, &a, &b), None);
}

#[test]
fn consolidate_leaves_deliberate_links_untouched() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["x"]);
    let b = seed(&conn, &["y"]);
    crate::test_support::link(&conn, a, "similar_to", b);
    consolidate(&conn, 30, Duration::seconds(2), 0.1, 5).unwrap();
    let links = db::all_memory_links(&conn).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].relation, "similar_to");
}

#[test]
fn consolidate_still_sees_a_spread_surfaced_memory() {
    let conn = db::open_in_memory_for_test();
    let a = seed(&conn, &["x"]);
    let b = seed(&conn, &["y"]);

    for _ in 0..2 {
        db::record_memory_recall(&conn, &a).unwrap();
        db::record_memory_surfaced(&conn, &b).unwrap();
    }

    let reinforced = consolidate(&conn, 30, Duration::seconds(2), 0.1, 5).unwrap();
    assert_eq!(
        reinforced, 1,
        "the surfaced memory must co-fire with the seed"
    );

    let g = MemoryGraph::build(&conn).unwrap();
    assert!(
        g.edge_weight(&a, &b).is_some(),
        "a spread-surfaced memory must still wire a co-activation edge"
    );
}
