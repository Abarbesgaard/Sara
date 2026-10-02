use std::collections::HashMap;

use crate::commands::reflect::types::UnionFind;

pub(in crate::commands::reflect) fn split_component(
    members: Vec<usize>,
    edges: &[(usize, usize, f64)],
    max: usize,
) -> Vec<Vec<usize>> {
    if members.len() <= max {
        return vec![members];
    }
    let local: HashMap<usize, usize> = members.iter().enumerate().map(|(k, &i)| (i, k)).collect();
    let inside: Vec<(usize, usize, f64)> = edges
        .iter()
        .copied()
        .filter(|(a, b, _)| local.contains_key(a) && local.contains_key(b))
        .collect();
    let Some(weakest) = inside.iter().map(|e| e.2).min_by(f64::total_cmp) else {
        return members.into_iter().map(|m| vec![m]).collect();
    };
    let kept: Vec<(usize, usize, f64)> = inside.into_iter().filter(|e| e.2 > weakest).collect();

    let mut uf = UnionFind::new(members.len());
    for (a, b, _) in &kept {
        uf.union(local[a], local[b]);
    }
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (k, &m) in members.iter().enumerate() {
        groups.entry(uf.find(k)).or_default().push(m);
    }
    let mut out = Vec::new();
    for group in groups.into_values() {
        out.extend(split_component(group, &kept, max));
    }
    out
}
