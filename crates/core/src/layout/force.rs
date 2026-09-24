/// A link pulling two nodes toward `length` apart with the given `strength` (0..1).
#[derive(Clone, Copy, Debug)]
pub struct Link {
    pub a: u32,
    pub b: u32,
    pub strength: f32,
    pub length: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct ForceParams {
    pub charge: f32,
    pub theta: f32,
    pub gravity: f32,
    pub velocity_decay: f32,
    pub iterations: u32,
    pub alpha: f32,
    pub alpha_min: f32,
}

impl ForceParams {
    pub const COLD: ForceParams =
        ForceParams { charge: -30.0, theta: 0.9, gravity: 0.02, velocity_decay: 0.4, iterations: 300, alpha: 1.0, alpha_min: 0.001 };
    /// A short, gentle run from the previous keyframe's positions, so nodes drift rather than reshuffle.
    pub const WARM: ForceParams = ForceParams { iterations: 60, alpha: 0.25, ..ForceParams::COLD };
}

const EMPTY: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Quad {
    x: f32,
    y: f32,
    mass: f32,
    size: f32,
    /// Index of the first of four children, or EMPTY for a leaf.
    children: u32,
    /// The single body in a leaf, or EMPTY.
    body: u32,
}

/// Barnes-Hut quadtree in a reusable arena, rebuilt each iteration without allocating.
#[derive(Default)]
pub struct Tree {
    quads: Vec<Quad>,
}

impl Tree {
    fn build(&mut self, pos: &[[f32; 2]]) {
        self.quads.clear();
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in pos {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let size = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0) * 1.001;
        self.quads.push(Quad { x: lo[0], y: lo[1], mass: 0.0, size, children: EMPTY, body: EMPTY });
        for i in 0..pos.len() {
            self.insert(0, i as u32, pos, 0);
        }
        self.summarize(0, pos);
    }

    fn child(&self, q: u32, p: [f32; 2]) -> u32 {
        let quad = self.quads[q as usize];
        let half = quad.size / 2.0;
        let right = u32::from(p[0] >= quad.x + half);
        let below = u32::from(p[1] >= quad.y + half);
        quad.children + right + 2 * below
    }

    fn split(&mut self, q: u32) {
        let quad = self.quads[q as usize];
        let half = quad.size / 2.0;
        let first = self.quads.len() as u32;
        for k in 0..4 {
            let (dx, dy) = ((k % 2) as f32 * half, (k / 2) as f32 * half);
            self.quads.push(Quad { x: quad.x + dx, y: quad.y + dy, mass: 0.0, size: half, children: EMPTY, body: EMPTY });
        }
        self.quads[q as usize].children = first;
    }

    fn insert(&mut self, q: u32, body: u32, pos: &[[f32; 2]], depth: u32) {
        let quad = self.quads[q as usize];
        if quad.children != EMPTY {
            let c = self.child(q, pos[body as usize]);
            return self.insert(c, body, pos, depth + 1);
        }
        if quad.body == EMPTY {
            self.quads[q as usize].body = body;
            return;
        }
        // Coincident points would recurse forever; past a depth they share a leaf and repel via the exact pass.
        if depth > 24 {
            return;
        }
        let existing = quad.body;
        self.quads[q as usize].body = EMPTY;
        self.split(q);
        let c = self.child(q, pos[existing as usize]);
        self.insert(c, existing, pos, depth + 1);
        let c = self.child(q, pos[body as usize]);
        self.insert(c, body, pos, depth + 1);
    }

    fn summarize(&mut self, q: u32, pos: &[[f32; 2]]) {
        let quad = self.quads[q as usize];
        let (mut mass, mut x, mut y) = (0.0, 0.0, 0.0);
        if quad.children == EMPTY {
            if quad.body != EMPTY {
                (mass, x, y) = (1.0, pos[quad.body as usize][0], pos[quad.body as usize][1]);
            }
        } else {
            for c in quad.children..quad.children + 4 {
                self.summarize(c, pos);
                let child = self.quads[c as usize];
                mass += child.mass;
                x += child.x * child.mass;
                y += child.y * child.mass;
            }
            if mass > 0.0 {
                (x, y) = (x / mass, y / mass);
            }
        }
        let quad = &mut self.quads[q as usize];
        // After summarizing, x/y hold the center of mass rather than the corner.
        (quad.mass, quad.x, quad.y) = (mass, x, y);
    }

    fn repel(&self, q: u32, i: usize, pos: &[[f32; 2]], params: &ForceParams, alpha: f32, v: &mut [f32; 2]) {
        let quad = self.quads[q as usize];
        let leaf = quad.children == EMPTY;
        if quad.mass == 0.0 || (leaf && quad.body == i as u32) {
            return;
        }
        let (dx, dy) = (quad.x - pos[i][0], quad.y - pos[i][1]);
        let d2 = (dx * dx + dy * dy).max(1.0);
        // Far enough away (relative to its size), a whole quad acts as one body at its center of mass.
        if leaf || quad.size * quad.size < params.theta * params.theta * d2 {
            let w = params.charge * alpha * quad.mass / d2;
            v[0] += dx * w;
            v[1] += dy * w;
            return;
        }
        for c in quad.children..quad.children + 4 {
            self.repel(c, i, pos, params, alpha, v);
        }
    }
}

/// Runs a d3-force style simulation (many-body, links, gravity) in place.
pub fn simulate(pos: &mut [[f32; 2]], links: &[Link], params: &ForceParams, tree: &mut Tree) {
    let n = pos.len();
    if n == 0 {
        return;
    }
    let mut vel = vec![[0.0f32; 2]; n];
    let mut degree = vec![0u32; n];
    for l in links {
        degree[l.a as usize] += 1;
        degree[l.b as usize] += 1;
    }
    let decay = 1.0 - (params.alpha_min / params.alpha).powf(1.0 / params.iterations as f32);
    let mut alpha = params.alpha;
    for _ in 0..params.iterations {
        for l in links {
            let (a, b) = (l.a as usize, l.b as usize);
            let dx = pos[b][0] + vel[b][0] - pos[a][0] - vel[a][0];
            let dy = pos[b][1] + vel[b][1] - pos[a][1] - vel[a][1];
            let d = (dx * dx + dy * dy).sqrt().max(1e-3);
            let f = (d - l.length) / d * alpha * l.strength;
            // Like d3, the lower-degree end moves more so hubs stay put.
            let bias = degree[a] as f32 / (degree[a] + degree[b]) as f32;
            vel[b][0] -= dx * f * bias;
            vel[b][1] -= dy * f * bias;
            vel[a][0] += dx * f * (1.0 - bias);
            vel[a][1] += dy * f * (1.0 - bias);
        }
        tree.build(pos);
        for i in 0..n {
            let mut v = [0.0; 2];
            tree.repel(0, i, pos, params, alpha, &mut v);
            vel[i][0] += v[0] - pos[i][0] * params.gravity * alpha;
            vel[i][1] += v[1] - pos[i][1] * params.gravity * alpha;
        }
        for (p, v) in pos.iter_mut().zip(&mut vel) {
            v[0] *= 1.0 - params.velocity_decay;
            v[1] *= 1.0 - params.velocity_decay;
            p[0] += v[0];
            p[1] += v[1];
        }
        alpha -= alpha * decay;
    }
}

/// Deterministic pseudo-random offset in [-1, 1]² from a key, for seeding new nodes.
pub fn jitter(key: u32) -> [f32; 2] {
    let mut h = u64::from(key).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    let a = (h & 0xFFFF) as f32 / 32767.5 - 1.0;
    let b = ((h >> 16) & 0xFFFF) as f32 / 32767.5 - 1.0;
    [a, b]
}

/// Phyllotaxis spiral: an even, deterministic cold-start spread.
pub fn phyllotaxis(i: usize) -> [f32; 2] {
    let r = 10.0 * (0.5 + i as f32).sqrt();
    let angle = i as f32 * std::f32::consts::PI * (3.0 - 5f32.sqrt());
    [r * angle.cos(), r * angle.sin()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clusters() -> (Vec<[f32; 2]>, Vec<Link>) {
        // Co-change graphs are clique-heavy (every pair in a commit links): two cliques of 20 and one bridge.
        let mut links = Vec::new();
        for c in 0..2u32 {
            for i in 0..20 {
                for j in i + 1..20 {
                    links.push(Link { a: c * 20 + i, b: c * 20 + j, strength: 0.8, length: 30.0 });
                }
            }
        }
        links.push(Link { a: 0, b: 20, strength: 0.2, length: 60.0 });
        ((0..40).map(phyllotaxis).collect(), links)
    }

    #[test]
    fn deterministic_and_finite() {
        let (mut a, links) = clusters();
        let mut b = a.clone();
        simulate(&mut a, &links, &ForceParams::COLD, &mut Tree::default());
        simulate(&mut b, &links, &ForceParams::COLD, &mut Tree::default());
        assert_eq!(a, b);
        assert!(a.iter().all(|p| p[0].is_finite() && p[1].is_finite()));
    }

    #[test]
    fn separates_clusters() {
        let (mut pos, links) = clusters();
        simulate(&mut pos, &links, &ForceParams::COLD, &mut Tree::default());
        let centroid = |r: std::ops::Range<usize>| {
            let n = r.len() as f32;
            let (x, y) = r.clone().fold((0.0, 0.0), |(x, y), i| (x + pos[i][0], y + pos[i][1]));
            let c = [x / n, y / n];
            let radius = r.map(|i| ((pos[i][0] - c[0]).powi(2) + (pos[i][1] - c[1]).powi(2)).sqrt()).sum::<f32>() / n;
            (c, radius)
        };
        let ((a, ra), (b, rb)) = (centroid(0..20), centroid(20..40));
        let gap = ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt();
        assert!(gap > 2.0 * (ra + rb), "centroids {gap} apart, radii {ra} + {rb}");
    }

    #[test]
    fn barnes_hut_matches_exact() {
        let pos: Vec<[f32; 2]> = (0..200)
            .map(|i| {
                let j = jitter(i);
                [j[0] * 300.0, j[1] * 300.0]
            })
            .collect();
        let params = ForceParams::COLD;
        let mut tree = Tree::default();
        tree.build(&pos);
        let mut worst = 0.0f32;
        for i in 0..pos.len() {
            let mut bh = [0.0; 2];
            tree.repel(0, i, &pos, &params, 1.0, &mut bh);
            let mut exact = [0.0f32; 2];
            for j in 0..pos.len() {
                if i == j {
                    continue;
                }
                let (dx, dy) = (pos[j][0] - pos[i][0], pos[j][1] - pos[i][1]);
                let w = params.charge / (dx * dx + dy * dy).max(1.0);
                exact[0] += dx * w;
                exact[1] += dy * w;
            }
            let err = ((bh[0] - exact[0]).powi(2) + (bh[1] - exact[1]).powi(2)).sqrt() / (exact[0].hypot(exact[1])).max(1e-6);
            worst = worst.max(err);
        }
        assert!(worst < 0.25, "worst relative error {worst}");
    }

    #[test]
    fn links_reach_their_length() {
        let mut pos = vec![[0.0, 0.0], [100.0, 0.0]];
        let links = [Link { a: 0, b: 1, strength: 1.0, length: 30.0 }];
        simulate(&mut pos, &links, &ForceParams { charge: 0.0, gravity: 0.0, ..ForceParams::COLD }, &mut Tree::default());
        assert!((pos[1][0] - pos[0][0] - 30.0).abs() < 0.5);
    }

    #[test]
    fn warm_start_moves_little() {
        let (mut pos, links) = clusters();
        let mut tree = Tree::default();
        simulate(&mut pos, &links, &ForceParams::COLD, &mut tree);
        let before = pos.clone();
        simulate(&mut pos, &links, &ForceParams::WARM, &mut tree);
        let drift = pos.iter().zip(&before).map(|(p, q)| (p[0] - q[0]).abs() + (p[1] - q[1]).abs()).sum::<f32>() / pos.len() as f32;
        assert!(drift < 5.0, "drift {drift}");
    }
}
