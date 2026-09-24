use rustc_hash::FxHashMap;

use crate::layout::{ForceParams, Link, Tree, jitter, phyllotaxis, simulate};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GraphEdge {
    pub a: u32,
    pub b: u32,
    pub count: u32,
    /// 0..1: how tightly the pair belongs together; pulls harder and sits closer when high.
    pub weight: f32,
}

/// One keyframe of a node-link view, laid out. Node ids are stable across keyframes.
#[derive(Clone, Default, Debug)]
pub struct GraphFrame {
    pub nodes: Vec<u32>,
    pub pos: Vec<[f32; 2]>,
    /// Drives node radius (commits for coupling, lines of code for dependencies).
    pub size: Vec<u32>,
    /// Path id to label the node with.
    pub path: Vec<u32>,
    pub group: Vec<u32>,
    pub dir: Vec<bool>,
    pub edges: Vec<GraphEdge>,
}

/// Remembers where every node was last drawn so later layouts warm-start from it.
pub struct Memory {
    positions: FxHashMap<u32, [f32; 2]>,
    tree: Tree,
    charge: f32,
    gravity: f32,
}

impl Default for Memory {
    fn default() -> Self {
        Memory::with_forces(ForceParams::COLD.charge, ForceParams::COLD.gravity)
    }
}

impl Memory {
    pub fn with_forces(charge: f32, gravity: f32) -> Self {
        Memory { positions: FxHashMap::default(), tree: Tree::default(), charge, gravity }
    }

    pub fn get(&self, node: u32) -> Option<[f32; 2]> {
        self.positions.get(&node).copied()
    }

    pub fn known(&self) -> impl Iterator<Item = (u32, [f32; 2])> + '_ {
        self.positions.iter().map(|(&k, &v)| (k, v))
    }

    /// Lays out `frame.nodes` with `frame.edges`, filling `frame.pos`. New nodes start at `hint` if it has one,
    /// else at the centroid of placed neighbours, nudged apart deterministically.
    pub fn place(&mut self, frame: &mut GraphFrame, hint: impl Fn(u32, &Memory) -> Option<[f32; 2]>) {
        let index: FxHashMap<u32, u32> = frame.nodes.iter().enumerate().map(|(i, &n)| (n, i as u32)).collect();
        let links: Vec<Link> = frame
            .edges
            .iter()
            .map(|e| Link { a: index[&e.a], b: index[&e.b], strength: e.weight, length: 18.0 + 50.0 * (1.0 - e.weight) })
            .collect();
        let known = frame.nodes.iter().filter(|n| self.positions.contains_key(n)).count();
        let mut pos: Vec<Option<[f32; 2]>> = frame.nodes.iter().map(|&n| self.get(n).or_else(|| hint(n, self))).collect();
        for (i, &n) in frame.nodes.iter().enumerate() {
            if pos[i].is_some() {
                continue;
            }
            let (mut sum, mut count) = ([0.0f32; 2], 0.0);
            for l in links.iter().filter(|l| l.a as usize == i || l.b as usize == i) {
                if let Some(p) = self.get(frame.nodes[(l.a + l.b) as usize - i]) {
                    (sum[0], sum[1], count) = (sum[0] + p[0], sum[1] + p[1], count + 1.0);
                }
            }
            if count > 0.0 {
                let j = jitter(n);
                pos[i] = Some([sum[0] / count + j[0] * 8.0, sum[1] / count + j[1] * 8.0]);
            }
        }
        let mut pos: Vec<[f32; 2]> = pos.into_iter().enumerate().map(|(i, p)| p.unwrap_or_else(|| phyllotaxis(i))).collect();
        let base = if known * 2 >= frame.nodes.len() && known > 0 { ForceParams::WARM } else { ForceParams::COLD };
        let params = ForceParams { charge: self.charge, gravity: self.gravity, ..base };
        simulate(&mut pos, &links, &params, &mut self.tree);
        for (&n, &p) in frame.nodes.iter().zip(&pos) {
            self.positions.insert(n, p);
        }
        frame.pos = pos;
    }
}

/// Two keyframes merged for tweening; nodes and edges present on one side only fade in or out in place.
/// Per-node and per-edge values are `[A, B]` pairs, zero where absent.
#[derive(Default)]
pub struct GraphSegment {
    pub ids: Vec<u32>,
    pub path: Vec<u32>,
    pub pos: Vec<f32>,
    pub size: Vec<u32>,
    pub group: Vec<u32>,
    pub dir: Vec<u8>,
    /// Endpoint node indices per edge, `a` then `b` (for dependencies: importer then imported).
    pub ends: Vec<u32>,
    pub count: Vec<u32>,
    pub weight: Vec<f32>,
}

/// The same edge at keyframe A and at keyframe B.
type EdgePair<'a> = (Option<&'a GraphEdge>, Option<&'a GraphEdge>);

pub fn merge(a: &GraphFrame, b: &GraphFrame, directed: bool) -> GraphSegment {
    let index_of = |f: &GraphFrame| f.nodes.iter().enumerate().map(|(i, &n)| (n, i)).collect::<FxHashMap<_, _>>();
    let (ia, ib) = (index_of(a), index_of(b));
    let mut ids: Vec<u32> = a.nodes.iter().chain(&b.nodes).copied().collect();
    ids.sort_unstable();
    ids.dedup();
    let index: FxHashMap<u32, u32> = ids.iter().enumerate().map(|(i, &n)| (n, i as u32)).collect();

    let mut s = GraphSegment::default();
    for &n in &ids {
        let (xa, xb) = (ia.get(&n).copied(), ib.get(&n).copied());
        let pa = xa.map(|i| a.pos[i]);
        let pb = xb.map(|i| b.pos[i]);
        let (pa, pb) = (pa.or(pb).unwrap(), pb.or(pa).unwrap());
        let any = |f: fn(&GraphFrame, usize) -> u32| xb.map(|i| f(b, i)).or(xa.map(|i| f(a, i))).unwrap();
        s.ids.push(n);
        s.pos.extend([pa[0], pa[1], pb[0], pb[1]]);
        s.path.extend([xa.map_or(0, |i| a.path[i]), xb.map_or(0, |i| b.path[i])]);
        s.size.extend([xa.map_or(0, |i| a.size[i]), xb.map_or(0, |i| b.size[i])]);
        s.group.push(any(|f, i| f.group[i]));
        s.dir.push(any(|f, i| u32::from(f.dir[i])) as u8);
    }

    let key = |e: &GraphEdge| if directed { (e.a, e.b) } else { (e.a.min(e.b), e.a.max(e.b)) };
    let mut edges: FxHashMap<(u32, u32), EdgePair> = FxHashMap::default();
    for e in &a.edges {
        edges.entry(key(e)).or_default().0 = Some(e);
    }
    for e in &b.edges {
        edges.entry(key(e)).or_default().1 = Some(e);
    }
    let mut pairs: Vec<_> = edges.into_iter().collect();
    pairs.sort_unstable_by_key(|(k, _)| *k);
    for ((x, y), (ea, eb)) in pairs {
        s.ends.extend([index[&x], index[&y]]);
        s.count.extend([ea.map_or(0, |e| e.count), eb.map_or(0, |e| e.count)]);
        s.weight.extend([ea.map_or(0.0, |e| e.weight), eb.map_or(0.0, |e| e.weight)]);
    }
    s
}
