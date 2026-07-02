//! Integration tests for `sara_tasks::commands::list`.
//! Moved out of an inline mod tests block in src/commands/list/mod.rs.

use sara_tasks::commands::list::LinkBadge;
use sara_tasks::infrastructure::db;

#[test]
fn link_badge_precedence_pr_beats_issue_beats_generic_link() {
    assert_eq!(
        LinkBadge::from_flags(db::LinkFlags {
            any: true,
            pr: true,
            issue: true,
        }),
        LinkBadge::Pr
    );
    assert_eq!(
        LinkBadge::from_flags(db::LinkFlags {
            any: true,
            pr: false,
            issue: true,
        }),
        LinkBadge::Issue
    );
    assert_eq!(
        LinkBadge::from_flags(db::LinkFlags {
            any: true,
            pr: false,
            issue: false,
        }),
        LinkBadge::Link
    );
    assert_eq!(
        LinkBadge::from_flags(db::LinkFlags::default()),
        LinkBadge::None
    );
}
