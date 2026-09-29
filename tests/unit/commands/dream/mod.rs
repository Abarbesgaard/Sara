use super::*;

#[test]
fn navigate_back_skips_dead_crumbs_and_lands_on_a_live_one() {
    use crate::infrastructure::model::Item;
    let conn = db::open_in_memory_for_test();
    let seed = |body: &str| -> Item {
        let mut item = Item::new_memory("t".into(), body.into(), None);
        item.path = Some(String::new());
        db::insert_item(&conn, &mut item).unwrap();
        item
    };

    let live = seed("still here");
    let dead = seed("about to be forgotten");
    let live_label = item_label(&live);
    let dead_label = item_label(&dead);

    db::archive_item(&conn, &dead.uuid).unwrap();

    let mut breadcrumb = vec![live_label.clone(), dead_label];
    let landed = navigate_back(&conn, &mut breadcrumb)
        .expect("should skip the dead crumb and reach the live one");
    assert_eq!(landed.item.uuid, live.uuid);
    assert!(
        breadcrumb.is_empty(),
        "both crumbs consumed: the dead one skipped, the live one landed on"
    );

    assert!(navigate_back(&conn, &mut breadcrumb).is_none());
}

#[test]
fn materialization_starts_noisy_and_fully_resolves() {
    let body = "the quick brown fox";
    let early = materialized_body(body, 0, 1.0);
    let noisy = early.iter().filter(|(_, real)| !real).count();
    assert!(noisy > 0, "frame 0 should still contain noise");

    let late = materialized_body(body, 10_000, 1.0);
    assert!(late.iter().all(|(_, real)| *real), "must fully resolve");
    let text: String = late.iter().map(|(c, _)| c).collect();
    assert_eq!(text, body);
}

#[test]
fn strong_memories_resolve_faster_than_weak() {
    let frame = 40;
    assert!(resolve_progress(frame, 2.5) > resolve_progress(frame, 1.0));
    assert!((resolve_progress(10_000, 1.0) - 1.0).abs() < f64::EPSILON);
}

#[test]
fn strength_labels_match_thresholds() {
    assert_eq!(strength_label(2.0), "Strong");
    assert_eq!(strength_label(1.5), "Linked");
    assert_eq!(strength_label(1.49), "Weak");
}

#[test]
fn noise_is_deterministic() {
    assert_eq!(noise(42), noise(42));
    assert_ne!(noise(1), noise(2));
}

#[test]
fn force_layout_pulls_bonded_stars_closer_than_strangers() {
    let mk = |tags: &[&str]| Star {
        label: "m1".into(),
        title: String::new(),
        strength: 1.0,
        provisional: false,
        tags: tags.iter().map(|t| t.to_string()).collect(),
        haystack: String::new(),
        x: 0.0,
        y: 0.0,
        recently_recalled: false,
    };
    let mut stars = vec![mk(&["a"]), mk(&["a"]), mk(&["b"]), mk(&["c"])];
    let affinity = vec![(0usize, 1usize, 0.02f64)];
    force_layout(&mut stars, &affinity, 250);
    let d = |i: usize, j: usize| {
        let (dx, dy) = (stars[i].x - stars[j].x, stars[i].y - stars[j].y);
        (dx * dx + dy * dy).sqrt()
    };
    assert!(
        d(0, 1) < d(2, 3),
        "sprung pair should sit closer than strangers"
    );
    assert!(stars.iter().all(|s| s.x.abs() <= 95.0 && s.y.abs() <= 68.0));
}

#[test]
fn affinity_springs_derive_from_calibrated_graph_weights() {
    use crate::infrastructure::model::Item;
    let conn = db::open_in_memory_for_test();
    let seed = |tags: &[&str]| -> String {
        let mut item = Item::new_memory("t".into(), "b".into(), None);
        item.tags = tags.iter().map(|s| s.to_string()).collect();
        item.path = Some(String::new());
        db::insert_item(&conn, &mut item).unwrap();
        item.uuid.to_string()
    };
    let a = seed(&["common", "rare"]);
    let b = seed(&["rare"]);
    let c = seed(&["common"]);
    for _ in 0..6 {
        seed(&["common"]);
    }

    let mut index = HashMap::new();
    for (i, m) in db::list_memories(&conn).unwrap().iter().enumerate() {
        index.insert(m.uuid.to_string(), i);
    }
    let graph = MemoryGraph::build(&conn).unwrap();
    let affinity: Vec<(usize, usize, f64)> = graph_edges(&graph, &index)
        .into_iter()
        .map(|(a, b, w)| (a, b, spring_stiffness(w)))
        .collect();

    let spring = |u: &str, v: &str| -> f64 {
        let (iu, iv) = (index[u], index[v]);
        affinity
            .iter()
            .find(|(i, j, _)| (*i == iu && *j == iv) || (*i == iv && *j == iu))
            .map(|(_, _, k)| *k)
            .unwrap_or(0.0)
    };
    let rare_pair = spring(&a, &b);
    let common_pair = spring(&a, &c);
    assert!(rare_pair > 0.0, "rare-anchor pair must keep a real spring");
    assert!(
        rare_pair > common_pair,
        "rare shared anchor ({rare_pair}) must bind tighter than ubiquitous ({common_pair}); \
             ubiquitous anchors below MIN_EDGE are dropped to 0 to de-clump the web",
    );
}

#[test]
fn shared_anchor_pairs_become_association_threads() {
    use crate::infrastructure::model::Item;
    let conn = db::open_in_memory_for_test();
    let seed = |tags: &[&str]| {
        let mut item = Item::new_memory("t".into(), "b".into(), None);
        item.tags = tags.iter().map(|s| s.to_string()).collect();
        item.path = Some(String::new());
        db::insert_item(&conn, &mut item).unwrap();
    };
    seed(&["rare"]);
    seed(&["rare"]);
    for _ in 0..6 {
        seed(&["filler"]);
    }

    let web = load_web(&conn).unwrap();
    assert!(web.bonds.is_empty(), "no explicit links were authored");
    assert!(
        !web.links.is_empty(),
        "a shared-anchor association must surface as a drawable thread even without a bond",
    );
    assert!(
        web.links.iter().all(|l| l.weight >= MIN_EDGE),
        "only real-signal associations (>= MIN_EDGE) are kept as threads",
    );
}

#[test]
fn bond_exists_matches_either_direction() {
    let bonds = vec![Bond {
        a: 1,
        b: 4,
        relation: "similar_to".into(),
    }];
    assert!(bond_exists(&bonds, 1, 4));
    assert!(bond_exists(&bonds, 4, 1));
    assert!(!bond_exists(&bonds, 1, 2));
}

#[test]
fn nearest_in_direction_picks_the_star_that_way() {
    let mk = |x: f64, y: f64| Star {
        label: String::new(),
        title: String::new(),
        strength: 1.0,
        provisional: false,
        tags: vec![],
        haystack: String::new(),
        x,
        y,
        recently_recalled: false,
    };
    let stars = vec![
        mk(0.0, 0.0),
        mk(10.0, 0.0),
        mk(-10.0, 0.0),
        mk(0.0, 10.0),
        mk(0.0, -10.0),
    ];
    assert_eq!(nearest_in_direction(&stars, 0, Dir::Right), 1);
    assert_eq!(nearest_in_direction(&stars, 0, Dir::Left), 2);
    assert_eq!(nearest_in_direction(&stars, 0, Dir::Up), 3);
    assert_eq!(nearest_in_direction(&stars, 0, Dir::Down), 4);
    assert_eq!(nearest_in_direction(&stars, 1, Dir::Right), 1);
    let stars2 = vec![mk(0.0, 0.0), mk(5.0, 1.0), mk(6.0, 40.0)];
    assert_eq!(nearest_in_direction(&stars2, 0, Dir::Right), 1);
}

#[test]
fn star_matches_searches_haystack_and_ignores_empty_query() {
    let star = Star {
        label: "m49".into(),
        title: "Usage-based strength".into(),
        strength: 1.0,
        provisional: false,
        tags: vec!["memory".into()],
        haystack: "m49 memory usage-based strength recall boost".into(),
        x: 0.0,
        y: 0.0,
        recently_recalled: false,
    };
    assert!(star.matches("recall"));
    assert!(star.matches("m49"));
    assert!(!star.matches("payment"));
    assert!(
        !star.matches(""),
        "empty query must not light everything up"
    );
}
