use super::types::Related;

/// Print a reconsider prompt for related prior findings, if any. No-op on empty.
pub fn print_related_findings(related: &[Related]) {
    if related.is_empty() {
        return;
    }
    eprintln!("⟳ reconsider — related prior finding(s) on this task:");
    for r in related {
        eprintln!("    (~{:.2}) #{}: {}", r.cosine, r.id, r.text);
    }
    eprintln!(
        "  If your new note revises or contradicts one, correct it (denotate / re-annotate)."
    );
}
