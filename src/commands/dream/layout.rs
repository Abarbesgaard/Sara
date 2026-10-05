use std::collections::HashMap;

use crate::infrastructure::memory::graph::MemoryGraph;

use super::types::{Bond, Dir, Star};

pub(super) fn bond_exists(bonds: &[Bond], a: usize, b: usize) -> bool {
    bonds
        .iter()
        .any(|bd| (bd.a == a && bd.b == b) || (bd.a == b && bd.b == a))
}

pub(super) fn nearest_in_direction(stars: &[Star], from: usize, dir: Dir) -> usize {
    let (ox, oy) = (stars[from].x, stars[from].y);
    let mut best = from;
    let mut best_score = f64::MAX;
    for (i, s) in stars.iter().enumerate() {
        if i == from {
            continue;
        }
        let (dx, dy) = (s.x - ox, s.y - oy);
        let (along, perp) = match dir {
            Dir::Right => (dx, dy.abs()),
            Dir::Left => (-dx, dy.abs()),
            Dir::Up => (dy, dx.abs()),
            Dir::Down => (-dy, dx.abs()),
        };
        if along <= 0.0 {
            continue;
        }
        let score = along + 2.0 * perp;
        if score < best_score {
            best_score = score;
            best = i;
        }
    }
    best
}

pub(super) const MIN_EDGE: f64 = 0.15;

pub(super) const CONTRAST: f64 = 3.0;

pub(super) const AFFINITY_SCALE: f64 = 0.12;

pub(super) const REPULSION: f64 = 190.0;

pub(super) const GRAVITY: f64 = 0.14;

pub(super) fn graph_edges(
    graph: &MemoryGraph,
    index: &HashMap<String, usize>,
) -> Vec<(usize, usize, f64)> {
    graph
        .edges()
        .into_iter()
        .filter(|(_, _, w)| *w >= MIN_EDGE)
        .filter_map(|(a, b, w)| {
            let ia = *index.get(&a.to_string())?;
            let ib = *index.get(&b.to_string())?;
            Some((ia, ib, w))
        })
        .collect()
}

pub(super) fn spring_stiffness(weight: f64) -> f64 {
    weight.powf(CONTRAST) * AFFINITY_SCALE
}

pub(super) fn force_layout(
    stars: &mut [Star],
    affinity: &[(usize, usize, f64)],
    iterations: usize,
) {
    let n = stars.len();
    if n < 2 {
        return;
    }
    for (i, s) in stars.iter_mut().enumerate() {
        let r = 70.0 * ((i + 1) as f64 / n as f64).sqrt();
        let a = i as f64 * 2.399_963;
        s.x = a.cos() * r * 1.3;
        s.y = a.sin() * r;
    }

    for _ in 0..iterations {
        let mut fx = vec![0.0f64; n];
        let mut fy = vec![0.0f64; n];
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = stars[i].x - stars[j].x;
                let dy = stars[i].y - stars[j].y;
                let d2 = (dx * dx + dy * dy).max(4.0);
                let rep = REPULSION / d2;
                let d = d2.sqrt();
                fx[i] += dx / d * rep;
                fy[i] += dy / d * rep;
                fx[j] -= dx / d * rep;
                fy[j] -= dy / d * rep;
            }
            fx[i] -= stars[i].x * GRAVITY;
            fy[i] -= stars[i].y * GRAVITY;
        }
        for &(i, j, k) in affinity {
            let dx = stars[j].x - stars[i].x;
            let dy = stars[j].y - stars[i].y;
            fx[i] += dx * k;
            fy[i] += dy * k;
            fx[j] -= dx * k;
            fy[j] -= dy * k;
        }
        for i in 0..n {
            stars[i].x = (stars[i].x + fx[i].clamp(-4.0, 4.0)).clamp(-95.0, 95.0);
            stars[i].y = (stars[i].y + fy[i].clamp(-4.0, 4.0)).clamp(-68.0, 68.0);
        }
    }
}
