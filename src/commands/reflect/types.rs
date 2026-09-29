#[derive(Debug)]
pub(super) struct Cluster {
    pub(super) members: Vec<String>,
    pub(super) suggested_canonical: String,
    pub(super) shared_tags: Vec<String>,
    pub(super) score: f64,
}

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
