use crate::commands::recall::types::Hit;

pub(in crate::commands::recall) fn match_confidence(
    query: &str,
    tags: &[String],
    hits: &[Hit],
) -> (&'static str, &'static str) {
    let has_query = !query.is_empty();
    let has_exact_filters = !tags.is_empty();

    if hits.is_empty() {
        if has_query {
            return (
                "none",
                "No keyword matches found. Sara uses literal FTS only — \
                 paraphrased or conceptually related content may not surface. \
                 Try --tag, different keywords, or --file to broaden the search.",
            );
        }
        return ("none", "");
    }

    let all_exact = hits.iter().all(|h| h.exact_match);
    if all_exact || (has_exact_filters && !has_query) {
        return ("high", "");
    }

    if hits.iter().any(|h| h.semantic) {
        return (
            "semantic",
            "Includes semantic matches (embedding similarity, marked *): these \
             may share no literal keyword with the query — verify relevance.",
        );
    }

    if hits.iter().any(|h| h.loose) {
        return (
            "low",
            "Loose match: only SOME query terms overlapped (token-OR fallback). \
             Results may be tangential and the best match need not be first — \
             refine the query or use --tag to narrow.",
        );
    }

    (
        "medium",
        "Keyword-match only (literal FTS). Paraphrased or conceptually \
         similar content with different wording may not appear.",
    )
}
