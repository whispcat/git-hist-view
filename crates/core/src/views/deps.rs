use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::graph::{GraphEdge, GraphFrame, Memory};
use crate::deps::{Import, Snapshot, parse_records, resolve};
use crate::git::{Kind, Oid, Store};
use crate::history::{History, PathId, Paths, ROOT, is_source, top_level};
use crate::layout::jitter;

/// Enough nodes to show structure, few enough to read labels.
pub const MAX_NODES: usize = 60;
const FRAME_CACHE: usize = 48;

/// Source files and their resolved imports at one commit.
pub struct Resolved {
    pub files: Vec<(PathId, u32)>,
    pub edges: Vec<(PathId, PathId)>,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct FrameKey {
    commit: u32,
    expanded: Vec<PathId>,
}

/// Parsed imports (per blob, so each version of a file is parsed once), resolutions and layouts.
pub struct Deps {
    imports: FxHashMap<Oid, Vec<Import>>,
    resolved: FxHashMap<u32, Rc<Resolved>>,
    frames: FxHashMap<FrameKey, GraphFrame>,
    memory: Memory,
}

impl Default for Deps {
    fn default() -> Self {
        // Import graphs have dense cores and many isolated files: push the core apart, pull stragglers in.
        Deps {
            imports: FxHashMap::default(),
            resolved: FxHashMap::default(),
            frames: FxHashMap::default(),
            memory: Memory::with_forces(-90.0, 0.06),
        }
    }
}

impl Deps {
    /// Source blobs live at `commit` whose imports haven't been extracted yet, with their paths.
    pub fn missing(&self, history: &History, commit: u32) -> Vec<(Oid, PathId)> {
        let mut out: Vec<(Oid, PathId)> = history
            .alive_at(commit)
            .filter(|(_, e)| !self.imports.contains_key(&e.blob) && is_source(&history.paths.get(e.path).name))
            .map(|(_, e)| (e.blob, e.path))
            .collect();
        out.sort_unstable();
        out.dedup_by_key(|(oid, _)| *oid);
        out
    }

    pub fn insert(&mut self, oid: Oid, records: &str) {
        self.imports.insert(oid, parse_records(records));
    }

    pub fn resolved(&mut self, history: &History, store: &mut Store, commit: u32) -> Rc<Resolved> {
        if let Some(r) = self.resolved.get(&commit) {
            return r.clone();
        }
        let live: Vec<(PathId, Oid, u32, String)> =
            history.alive_at(commit).map(|(_, e)| (e.path, e.blob, e.loc, history.paths.full(e.path))).collect();
        let by_path: FxHashMap<&str, (PathId, Oid)> = live.iter().map(|(p, o, _, s)| (s.as_str(), (*p, *o))).collect();
        let sources: FxHashMap<&str, &[Import]> = live
            .iter()
            .filter(|(p, ..)| is_source(&history.paths.get(*p).name))
            .map(|(_, oid, _, s)| (s.as_str(), self.imports.get(oid).map_or(&[][..], Vec::as_slice)))
            .collect();
        let snap = Snapshot { files: by_path.keys().copied().collect(), sources };
        let mut buf = Vec::new();
        let mut read = |path: &str| {
            let (_, oid) = by_path.get(path)?;
            store.read_as(oid, Kind::Blob, &mut buf).ok()?;
            Some(String::from_utf8_lossy(&buf).into_owned())
        };
        let edges = resolve(&snap, &mut read).into_iter().map(|(a, b)| (by_path[a].0, by_path[b].0)).collect();
        let files = live.iter().filter(|(p, ..)| is_source(&history.paths.get(*p).name)).map(|(p, _, loc, _)| (*p, *loc)).collect();
        let r = Rc::new(Resolved { files, edges });
        self.resolved.insert(commit, r.clone());
        r
    }

    /// The folder graph at `commit`: each file shown as its first ancestor folder that isn't `expanded`, laid out.
    pub fn frame(&mut self, history: &History, store: &mut Store, commit: u32, expanded: &[PathId]) -> GraphFrame {
        let key = FrameKey { commit, expanded: expanded.to_vec() };
        if let Some(f) = self.frames.get(&key) {
            return f.clone();
        }
        let resolved = self.resolved(history, store, commit);
        let paths = &history.paths;
        let open: FxHashSet<PathId> = expanded.iter().copied().collect();
        let mut index: FxHashMap<PathId, usize> = FxHashMap::default();
        let mut frame = GraphFrame::default();
        let mut node_of: FxHashMap<PathId, PathId> = FxHashMap::default();
        for &(file, loc) in &resolved.files {
            let node = collapse(paths, file, &open);
            node_of.insert(file, node);
            let i = *index.entry(node).or_insert_with(|| {
                frame.nodes.push(node);
                frame.size.push(0);
                frame.path.push(node);
                frame.group.push(top_level(paths, node));
                frame.dir.push(paths.get(node).is_dir);
                frame.nodes.len() - 1
            });
            frame.size[i] += loc;
        }
        let mut weights: FxHashMap<(PathId, PathId), u32> = FxHashMap::default();
        for (a, b) in &resolved.edges {
            let (na, nb) = (node_of[a], node_of[b]);
            if na != nb {
                *weights.entry((na, nb)).or_default() += 1;
            }
        }
        let mut edges: Vec<_> = weights.into_iter().collect();
        edges.sort_unstable();
        // Import graphs are hub-heavy; loose links keep hubs from pulling everything into one knot.
        frame.edges = edges
            .into_iter()
            .map(|((a, b), count)| GraphEdge { a, b, count, weight: 0.12 + 0.3 * count as f32 / (count as f32 + 5.0) })
            .collect();
        // An expanded folder's files burst out from where the folder was; a collapsed folder gathers its files.
        self.memory.place(&mut frame, |node, memory| {
            let mut at = paths.get(node).parent;
            while at != ROOT {
                if let Some(p) = memory.get(at) {
                    let j = jitter(node);
                    return Some([p[0] + j[0] * 10.0, p[1] + j[1] * 10.0]);
                }
                at = paths.get(at).parent;
            }
            let inside: Vec<[f32; 2]> = memory.known().filter(|&(k, _)| is_within(paths, k, node)).map(|(_, p)| p).collect();
            (!inside.is_empty()).then(|| {
                let n = inside.len() as f32;
                [inside.iter().map(|p| p[0]).sum::<f32>() / n, inside.iter().map(|p| p[1]).sum::<f32>() / n]
            })
        });
        if self.frames.len() >= FRAME_CACHE {
            self.frames.clear();
        }
        self.frames.insert(key, frame.clone());
        frame
    }

    /// Folders to expand so the graph shows as much structure as fits in `MAX_NODES`: repeatedly opens the
    /// largest folder that still fits, so detail goes where the code is rather than to a uniform depth.
    pub fn auto_expand(&mut self, history: &History, store: &mut Store, commit: u32) -> Vec<PathId> {
        let resolved = self.resolved(history, store, commit);
        let paths = &history.paths;
        let mut open: FxHashSet<PathId> = FxHashSet::default();
        let nodes = |open: &FxHashSet<PathId>| {
            let mut loc: FxHashMap<PathId, u64> = FxHashMap::default();
            for &(f, l) in &resolved.files {
                *loc.entry(collapse(paths, f, open)).or_default() += u64::from(l);
            }
            loc
        };
        loop {
            let current = nodes(&open);
            let mut folders: Vec<(PathId, u64)> = current.iter().filter(|(n, _)| paths.get(**n).is_dir).map(|(&n, &l)| (n, l)).collect();
            folders.sort_unstable_by_key(|&(n, l)| (std::cmp::Reverse(l), n));
            let next = folders.into_iter().find(|&(n, _)| {
                open.insert(n);
                let fits = nodes(&open).len() <= MAX_NODES;
                open.remove(&n);
                fits
            });
            match next {
                Some((n, _)) => open.insert(n),
                None => break,
            };
        }
        let mut out: Vec<PathId> = open.into_iter().collect();
        out.sort_unstable();
        out
    }
}

fn is_within(paths: &Paths, mut id: PathId, dir: PathId) -> bool {
    while id != ROOT {
        id = paths.get(id).parent;
        if id == dir {
            return true;
        }
    }
    false
}

/// The node a file belongs to: its first ancestor folder that isn't expanded, or the file itself.
pub fn collapse(paths: &Paths, file: PathId, expanded: &FxHashSet<PathId>) -> PathId {
    let mut chain = Vec::new();
    let mut at = file;
    while at != ROOT {
        chain.push(at);
        at = paths.get(at).parent;
    }
    chain.reverse();
    chain.into_iter().find(|p| paths.get(*p).is_dir && !expanded.contains(p)).unwrap_or(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapses_to_the_first_closed_folder() {
        let mut p = Paths::default();
        let src = p.child(ROOT, b"src", true);
        let core = p.child(src, b"core", true);
        let file = p.child(core, b"a.rs", false);
        let top = p.child(ROOT, b"build.rs", false);
        let none = FxHashSet::default();
        assert_eq!(collapse(&p, file, &none), src);
        assert_eq!(collapse(&p, top, &none), top);
        assert_eq!(collapse(&p, file, &[src].into_iter().collect()), core);
        assert_eq!(collapse(&p, file, &[src, core].into_iter().collect()), file);
    }
}
