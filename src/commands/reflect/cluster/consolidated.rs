use std::collections::{HashMap, HashSet};

pub(in crate::commands::reflect) fn is_already_consolidated(
    uuids: &[String],
    derived_parents: &HashMap<String, HashSet<String>>,
) -> bool {
    let mut candidates: HashSet<&String> = uuids.iter().collect();
    for u in uuids {
        if let Some(parents) = derived_parents.get(u) {
            candidates.extend(parents);
        }
    }
    candidates.into_iter().any(|p| {
        uuids.iter().all(|u| {
            u == p
                || derived_parents
                    .get(u)
                    .is_some_and(|parents| parents.contains(p))
        })
    })
}
