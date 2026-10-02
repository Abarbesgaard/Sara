use std::collections::{HashMap, HashSet};

pub(super) fn is_already_consolidated(
    uuids: &[String],
    derived_parents: &HashMap<String, HashSet<String>>,
) -> bool {
    let mut candidates: HashSet<String> = uuids.iter().cloned().collect();
    for u in uuids {
        if let Some(parents) = derived_parents.get(u) {
            candidates.extend(parents.iter().cloned());
        }
    }
    candidates.iter().any(|p| {
        uuids.iter().all(|u| {
            u == p
                || derived_parents
                    .get(u)
                    .is_some_and(|parents| parents.contains(p))
        })
    })
}
