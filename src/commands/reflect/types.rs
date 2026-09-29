/// A proposed consolidation: a cluster of related, not-yet-consolidated memories
/// and the canonical+derived_from restructuring that would tidy them.
#[derive(Debug)]
pub(super) struct Cluster {
    /// Member labels (e.g. `m209`), suggested canonical first.
    pub(super) members: Vec<String>,
    /// Label of the member proposed to become the canonical (strongest member).
    pub(super) suggested_canonical: String,
    /// Tags shared by every member (may be empty).
    pub(super) shared_tags: Vec<String>,
    /// Ranking score: size dominates, strength breaks ties.
    pub(super) score: f64,
}

/// Union-find over memory indices for transitive clustering.
pub(super) struct UnionFind {
    parent: Vec<usize>,
}

impl UnionFind {
    pub(super) fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n).collect(),
        }
    }
    pub(super) fn find(&mut self, x: usize) -> usize {
        let mut r = x;
        while self.parent[r] != r {
            r = self.parent[r];
        }
        // Path compression.
        let mut cur = x;
        while self.parent[cur] != r {
            let next = self.parent[cur];
            self.parent[cur] = r;
            cur = next;
        }
        r
    }
    pub(super) fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.parent[ra] = rb;
        }
    }
}
