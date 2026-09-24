use rustc_hash::FxHashMap;

use crate::history::{History, PathId, ROOT, language};
use crate::layout::{Rect, split, squarify};

const DIR_PAD: f32 = 2.0;
const HEADER: f32 = 16.0;
const HEADER_MIN_W: f32 = 64.0;
const HEADER_MIN_H: f32 = 40.0;
pub const NO_OWNER: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Split,
    Squarify,
}

#[derive(Clone, Copy, Debug)]
pub struct TreemapOptions {
    pub width: f32,
    pub height: f32,
    pub root: PathId,
    pub window_secs: i64,
    pub layout: Layout,
}

#[derive(Clone, Copy, Debug)]
pub struct Cell {
    pub id: PathId,
    pub rect: Rect,
    pub depth: u8,
    pub is_dir: bool,
    pub header: bool,
    pub loc: u32,
    pub churn: u32,
    pub owner: u32,
    pub lang: u8,
}

struct Node {
    children: Vec<PathId>,
    loc: u64,
    churn: u64,
    owner: u32,
    lang: u8,
}

/// Lays out every live file under `root` at `commit`, parents before children.
pub fn frame(history: &History, commit: u32, opts: &TreemapOptions) -> Vec<Cell> {
    let paths = &history.paths;
    let commits = &history.commits;
    let since = commits[commit as usize].time - opts.window_secs;
    let after = commits.partition_point(|c| c.time <= since).checked_sub(1).map(|i| i as u32);
    let is_under = |mut id: PathId| {
        while id != opts.root {
            if id == ROOT {
                return false;
            }
            id = paths.get(id).parent;
        }
        true
    };

    let mut nodes: FxHashMap<PathId, Node> = FxHashMap::default();
    let node = || Node { children: Vec::new(), loc: 0, churn: 0, owner: NO_OWNER, lang: 0 };
    nodes.insert(opts.root, node());
    for (file, e) in history.alive_at(commit) {
        if e.loc == 0 || !is_under(e.path) {
            continue;
        }
        let churn = history.churn_between(file, after.filter(|&a| a < commit), commit);
        let (loc, name) = (u64::from(e.loc), &paths.get(e.path).name);
        nodes.insert(e.path, Node { children: Vec::new(), loc, churn: churn.into(), owner: e.owner, lang: language(name) });
        let (mut child, mut child_new) = (e.path, true);
        while child != opts.root {
            let parent = paths.get(child).parent;
            let parent_new = !nodes.contains_key(&parent);
            let p = nodes.entry(parent).or_insert_with(node);
            p.loc += loc;
            p.churn += u64::from(churn);
            if child_new {
                p.children.push(child);
            }
            (child, child_new) = (parent, parent_new);
        }
    }
    for n in nodes.values_mut() {
        n.children.sort_unstable_by(|a, b| paths.get(*a).name.cmp(&paths.get(*b).name));
    }

    let mut out = Vec::with_capacity(nodes.len());
    let bounds = Rect { x: 0.0, y: 0.0, w: opts.width, h: opts.height };
    place(&nodes, opts.root, bounds, 0, opts.layout, &mut out);
    out
}

fn place(nodes: &FxHashMap<PathId, Node>, id: PathId, rect: Rect, depth: u8, layout: Layout, out: &mut Vec<Cell>) {
    let n = &nodes[&id];
    let weights: Vec<f64> = n.children.iter().map(|c| nodes[c].loc as f64).collect();
    let mut rects = Vec::with_capacity(weights.len());
    match layout {
        Layout::Split => split(&weights, rect, &mut rects),
        Layout::Squarify => squarify(&weights, rect, &mut rects),
    }
    for (&child, &r) in n.children.iter().zip(&rects) {
        let c = &nodes[&child];
        let is_dir = !c.children.is_empty();
        let header = is_dir && r.w >= HEADER_MIN_W && r.h >= HEADER_MIN_H;
        out.push(Cell {
            id: child,
            rect: r,
            depth,
            is_dir,
            header,
            loc: c.loc.min(u64::from(u32::MAX)) as u32,
            churn: c.churn.min(u64::from(u32::MAX)) as u32,
            owner: c.owner,
            lang: c.lang,
        });
        if is_dir {
            let pad = if r.w > 4.0 * DIR_PAD && r.h > 4.0 * DIR_PAD { DIR_PAD } else { 0.0 };
            let inner = r.inset(pad, pad + if header { HEADER - DIR_PAD } else { 0.0 }, pad, pad);
            place(nodes, child, inner, depth.saturating_add(1), layout, out);
        }
    }
}

/// Two frames merged by path id, packed for the GPU: cells present in only one frame
/// grow from (or shrink into) the center of their nearest ancestor in the other.
#[derive(Default)]
pub struct Segment {
    pub ids: Vec<u32>,
    /// x, y, w, h at A then at B.
    pub geom: Vec<f32>,
    pub loc: Vec<u32>,
    pub churn: Vec<u32>,
    pub owner: Vec<u32>,
    pub lang: Vec<u8>,
    /// bit 0: directory, bit 1: has header, bits 2..: depth.
    pub flags: Vec<u8>,
}

pub fn merge(a: &[Cell], b: &[Cell], paths: &crate::history::Paths, bounds: Rect) -> Segment {
    let index = |cells: &[Cell]| cells.iter().enumerate().map(|(i, c)| (c.id, i)).collect::<FxHashMap<_, _>>();
    let (ia, ib) = (index(a), index(b));
    let mut order: Vec<(u8, PathId, Option<usize>, Option<usize>)> =
        b.iter().enumerate().map(|(i, c)| (c.depth, c.id, ia.get(&c.id).copied(), Some(i))).collect();
    order.extend(a.iter().enumerate().filter(|(_, c)| !ib.contains_key(&c.id)).map(|(i, c)| (c.depth, c.id, Some(i), None)));
    order.sort_unstable_by_key(|&(depth, id, ..)| (depth, id));

    let anchor = |cells: &[Cell], index: &FxHashMap<PathId, usize>, mut id: PathId| loop {
        if id == ROOT {
            return bounds.center();
        }
        id = paths.get(id).parent;
        if let Some(&i) = index.get(&id) {
            return cells[i].rect.center();
        }
    };

    let mut s = Segment::default();
    for (depth, id, ai, bi) in order {
        let ca = ai.map(|i| a[i]);
        let cb = bi.map(|i| b[i]);
        let ra = ca.map_or_else(|| anchor(a, &ia, id), |c| c.rect);
        let rb = cb.map_or_else(|| anchor(b, &ib, id), |c| c.rect);
        let any = cb.or(ca).unwrap();
        s.ids.push(id);
        s.geom.extend([ra.x, ra.y, ra.w, ra.h, rb.x, rb.y, rb.w, rb.h]);
        s.loc.extend([ca.map_or(0, |c| c.loc), cb.map_or(0, |c| c.loc)]);
        s.churn.extend([ca.map_or(0, |c| c.churn), cb.map_or(0, |c| c.churn)]);
        s.owner.extend([ca.map_or(any.owner, |c| c.owner), cb.map_or(any.owner, |c| c.owner)]);
        s.lang.push(any.lang);
        s.flags.push(u8::from(any.is_dir) | u8::from(any.header) << 1 | depth.min(63) << 2);
    }
    s
}
