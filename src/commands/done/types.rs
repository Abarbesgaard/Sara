/// Verdict of the fail-closed validation gate that guards `done`. A task should
/// not close on prose: if it declares a definition of done (acceptance
/// criteria), those must have been proven green by `validate` against the
/// project's current HEAD before it can be completed.
pub(super) enum DoneGate {
    /// Validated and fresh (or no git HEAD to compare against) — close cleanly.
    Ok,
    /// The task has acceptance criteria that were never proven, or were proven
    /// at an earlier commit and HEAD has moved since. Block unless `force`.
    Refuse(String),
    /// The task has no acceptance criteria at all — nothing to prove. Allowed,
    /// but surfaced as an advisory so the missing definition of done is visible.
    Warn(String),
}
