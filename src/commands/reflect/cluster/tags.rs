use std::collections::{HashMap, HashSet};

pub(in crate::commands::reflect) fn shared_tags(
    uuids: &[String],
    tags_by_uuid: &HashMap<String, Vec<String>>,
) -> Vec<String> {
    let mut iter = uuids.iter();
    let first = match iter.next().and_then(|u| tags_by_uuid.get(u)) {
        Some(t) => t.clone(),
        None => return vec![],
    };
    let mut shared: Vec<String> = first;
    for u in iter {
        let set: HashSet<&String> = tags_by_uuid
            .get(u)
            .map(|t| t.iter().collect())
            .unwrap_or_default();
        shared.retain(|t| set.contains(t));
    }
    shared.sort();
    shared.dedup();
    shared
}
