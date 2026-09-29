use super::*;

#[test]
fn embedding_related_closer_than_unrelated() {
    let e = bundled();
    let anchor = e.embed("dependabot bump broke the restore step; pin the lockfile version");
    let related = e.embed("dependency update caused a CI build failure");
    let unrelated = e.embed("how to bake sourdough bread at home");
    let rel = cosine(&anchor, &related);
    let unrel = cosine(&anchor, &unrelated);
    assert!(
        rel > unrel,
        "related ({rel}) should exceed unrelated ({unrel})"
    );
}

#[test]
fn embedding_is_deterministic_normalized_and_sized() {
    let e = bundled();
    assert_eq!(e.dim(), 256);
    let a = e.embed("stacked PR auto-closed when its base branch was deleted");
    let b = e.embed("stacked PR auto-closed when its base branch was deleted");
    assert_eq!(a, b, "embedding must be deterministic");
    assert_eq!(a.len(), 256);
    let norm: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((norm - 1.0).abs() < 1e-4, "expected unit norm, got {norm}");
}

#[test]
fn embedding_empty_text_is_zero_vector() {
    let e = bundled();
    let v = e.embed("   ");
    assert_eq!(v.len(), 256);
    assert!(v.iter().all(|&x| x == 0.0));
}

#[test]
fn cosine_of_identical_is_one() {
    let e = bundled();
    let v = e.embed("reciprocal rank fusion merges lexical and semantic hits");
    assert!((cosine(&v, &v) - 1.0).abs() < 1e-4);
}

#[test]
fn scheme_version_changes_when_model_fingerprint_changes() {
    let a = compose_scheme_version("m2v/v1/n100/d256/s3dcccccd", MEMORY_EMBED_TEXT_VERSION);
    let b = compose_scheme_version("m2v/v2/n100/d256/s3dcccccd", MEMORY_EMBED_TEXT_VERSION);
    assert_ne!(a, b);
}

#[test]
fn scheme_version_changes_when_embed_text_version_changes() {
    let fp = "m2v/v1/n100/d256/s3dcccccd";
    assert_ne!(compose_scheme_version(fp, 1), compose_scheme_version(fp, 2));
}

#[test]
fn needs_reindex_only_on_missing_or_mismatched_version() {
    let current = "m2v/v1|text1";
    assert!(needs_reindex(None, current), "fresh DB must reindex");
    assert!(
        needs_reindex(Some("m2v/v1|text0"), current),
        "stale scheme must reindex"
    );
    assert!(
        !needs_reindex(Some(current), current),
        "matching scheme must NOT reindex"
    );
}

#[test]
fn fingerprint_is_stable_for_the_bundled_model() {
    assert_eq!(bundled().fingerprint(), bundled().fingerprint());
    assert_eq!(scheme_version(), scheme_version());
}

#[test]
fn ensure_index_current_reindexes_once_then_noops() {
    use crate::infrastructure::db;
    use crate::infrastructure::model::Item;

    let conn = db::open_in_memory_for_test();
    let mut mem = Item::new_memory(
        "lockfile pin".into(),
        "pin the dependency lockfile to stop dependabot breaking restore".into(),
        None,
    );
    mem.path = Some("memory/lockfile-pin.md".into());
    db::insert_item(&conn, &mut mem).unwrap();

    assert!(ensure_index_current(&conn).unwrap(), "first run must heal");
    assert_eq!(
        db::meta_get(&conn, SCHEME_VERSION_KEY).unwrap().as_deref(),
        Some(scheme_version().as_str()),
        "scheme version must be recorded after reindex"
    );
    assert!(
        db::get_embedding(&conn, &mem.uuid.to_string())
            .unwrap()
            .is_some(),
        "the seeded memory must have a vector after reindex"
    );

    assert!(
        !ensure_index_current(&conn).unwrap(),
        "second run must not reindex"
    );
}

#[test]
fn ensure_index_current_reindexes_after_a_scheme_bump() {
    use crate::infrastructure::db;

    let conn = db::open_in_memory_for_test();
    db::meta_set(&conn, SCHEME_VERSION_KEY, "stale-old-scheme").unwrap();

    assert!(
        ensure_index_current(&conn).unwrap(),
        "a scheme mismatch must force a reindex"
    );
    assert_eq!(
        db::meta_get(&conn, SCHEME_VERSION_KEY).unwrap().as_deref(),
        Some(scheme_version().as_str()),
    );
}
