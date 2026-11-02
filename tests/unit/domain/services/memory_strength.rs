use super::*;

#[test]
fn memory_strength_source_task_outranks_links() {
    let s = base_strength(
        MemoryStatus::Active,
        Some(&Status::Pending),
        &[Status::Completed],
    );
    assert_eq!(s, 1.5);
    assert_eq!(
        base_strength(MemoryStatus::Active, Some(&Status::Completed), &[]),
        2.0
    );
}

#[test]
fn memory_strength_falls_back_to_best_linked_task() {
    assert_eq!(base_strength(MemoryStatus::Active, None, &[]), 1.0);
    assert_eq!(
        base_strength(
            MemoryStatus::Active,
            None,
            &[Status::Deleted, Status::Pending]
        ),
        1.5
    );
    assert_eq!(
        base_strength(
            MemoryStatus::Active,
            None,
            &[Status::Pending, Status::Completed]
        ),
        2.0
    );
}

#[test]
fn memory_strength_caps_provisional_base() {
    let s = base_strength(MemoryStatus::Provisional, Some(&Status::Completed), &[]);
    assert_eq!(s, PROVISIONAL_BASE_CAP);
}

#[test]
fn memory_strength_bonuses_are_capped() {
    assert_eq!(recall_boost(2), 0.2);
    assert_eq!(recall_boost(100), RECALL_BOOST_CAP);
    assert_eq!(derived_bonus(3), 0.30000000000000004);
    assert_eq!(derived_bonus(9), DERIVED_BONUS_CAP);
}
