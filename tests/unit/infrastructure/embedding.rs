use super::*;

#[test]
fn embedding_related_closer_than_unrelated() {
    // The whole point of semantic recall: a paraphrase must be closer than
    // an unrelated sentence, even with different surface wording.
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
