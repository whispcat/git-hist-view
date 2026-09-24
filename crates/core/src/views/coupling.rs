use rustc_hash::FxHashMap;

use super::graph::{GraphEdge, GraphFrame, Memory};
use crate::history::{Edge, History, top_level};

/// Lays out coupling keyframes strictly in order, each warm-started from the positions before it, so the
/// same keyframe always gets the same picture no matter how the user got there.
#[derive(Default)]
pub struct Layouts {
    frames: Vec<GraphFrame>,
    memory: Memory,
}

impl Layouts {
    pub fn len(&self) -> usize {
        self.frames.len()
    }

    pub fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub fn get(&self, k: usize) -> Option<&GraphFrame> {
        self.frames.get(k)
    }

    /// Lays out the next keyframe (at `commit`) from its co-change edges.
    pub fn push(&mut self, history: &History, commit: u32, edges: &[Edge]) {
        let mut commits: FxHashMap<u32, u32> = FxHashMap::default();
        for e in edges {
            commits.insert(e.a, e.a_commits);
            commits.insert(e.b, e.b_commits);
        }
        let mut frame = GraphFrame { nodes: commits.keys().copied().collect(), ..Default::default() };
        frame.nodes.sort_unstable();
        for &f in &frame.nodes {
            let path = history.event_at(f, commit).filter(|e| e.alive()).map_or(0, |e| e.path);
            frame.size.push(commits[&f]);
            frame.path.push(path);
            frame.group.push(if path == 0 { 0 } else { top_level(&history.paths, path) });
            frame.dir.push(false);
        }
        frame.edges = edges.iter().map(|e| GraphEdge { a: e.a, b: e.b, count: e.count, weight: e.jaccard }).collect();
        self.memory.place(&mut frame, |_, _| None);
        self.frames.push(frame);
    }
}
