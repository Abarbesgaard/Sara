use super::*;

#[test]
fn link_badge_precedence_pr_beats_issue_beats_generic_link() {
    assert_eq!(
        LinkBadge::from_flags(
            db::LinkFlags {
                any: true,
                pr: true,
                issue: true,
            },
            true,
        ),
        LinkBadge::Pr
    );
    assert_eq!(
        LinkBadge::from_flags(
            db::LinkFlags {
                any: true,
                pr: false,
                issue: true,
            },
            true,
        ),
        LinkBadge::Issue
    );
    assert_eq!(
        LinkBadge::from_flags(
            db::LinkFlags {
                any: true,
                pr: false,
                issue: false,
            },
            true,
        ),
        LinkBadge::Link
    );
    assert_eq!(
        LinkBadge::from_flags(db::LinkFlags::default(), true),
        LinkBadge::None
    );
}

#[test]
fn link_badge_issue_link_without_sync_provenance_is_generic_link() {
    // A task that merely links back to an issue for traceability (e.g. a
    // subtask generated while breaking down an issue) is not itself an
    // imported task, so it should not wear the ISS badge.
    assert_eq!(
        LinkBadge::from_flags(
            db::LinkFlags {
                any: true,
                pr: false,
                issue: true,
            },
            false,
        ),
        LinkBadge::Link
    );
}
