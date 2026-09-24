use rustc_hash::{FxHashMap, FxHashSet};

use super::engine::{FileId, History};
use super::filters::MANIFESTS;

#[derive(Clone, Copy, Debug)]
pub struct CouplingOptions {
    pub window_secs: i64,
    pub min_support: u32,
    pub min_jaccard: f32,
    /// Commits touching more files than this (formatting, vendoring, mass renames) say nothing about coupling.
    pub max_files: usize,
    pub max_edges: usize,
    /// Dependency bots bump many files at once, which is bookkeeping rather than design coupling.
    pub ignore_bots: bool,
    /// Commits touching only dependency manifests (version bumps) likewise.
    pub ignore_manifest_only: bool,
}

impl Default for CouplingOptions {
    fn default() -> Self {
        CouplingOptions {
            window_secs: 182 * 86_400,
            min_support: 3,
            min_jaccard: 0.2,
            max_files: 50,
            max_edges: 400,
            ignore_bots: true,
            ignore_manifest_only: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub a: FileId,
    pub b: FileId,
    pub count: u32,
    pub jaccard: f32,
    /// Commits in the window touching `a` and `b` respectively.
    pub a_commits: u32,
    pub b_commits: u32,
}

fn key(a: FileId, b: FileId) -> u64 {
    (u64::from(a.min(b)) << 32) | u64::from(a.max(b))
}

/// Whether a commit says nothing about which files belong together.
pub fn ignored(history: &History, commit: usize, opts: &CouplingOptions) -> bool {
    let files = history.touched(commit);
    let c = &history.commits[commit];
    let manifest = |&f: &FileId| {
        let path = history.event_at(f, commit as u32).map(|e| e.path);
        path.is_some_and(|p| MANIFESTS.contains(&&*history.paths.get(p).name))
    };
    files.len() > opts.max_files
        || (opts.ignore_bots && history.authors.list[c.author as usize].bot)
        || (opts.ignore_manifest_only && !files.is_empty() && files.iter().all(manifest))
}

/// Co-change counts over a sliding time window, sampled at each keyframe as history streams in.
pub struct Coupling {
    opts: CouplingOptions,
    keyframes: Vec<u32>,
    pub edges: Vec<Vec<Edge>>,
    head: usize,
    tail: usize,
    files: FxHashMap<FileId, u32>,
    pairs: FxHashMap<u64, u32>,
    candidates: FxHashSet<u64>,
}

impl Coupling {
    pub fn new(keyframes: Vec<u32>, opts: CouplingOptions) -> Self {
        Coupling {
            opts,
            keyframes,
            edges: Vec::new(),
            head: 0,
            tail: 0,
            files: FxHashMap::default(),
            pairs: FxHashMap::default(),
            candidates: FxHashSet::default(),
        }
    }

    pub fn is_done(&self) -> bool {
        self.edges.len() == self.keyframes.len()
    }

    /// Consumes every keyframe that history has already processed.
    pub fn advance(&mut self, history: &History) {
        while let Some(&k) = self.keyframes.get(self.edges.len()).filter(|&&k| (k as usize) < history.frontier()) {
            for c in self.head..=k as usize {
                self.count(history, c, 1);
            }
            self.head = k as usize + 1;
            let start = history.commits[k as usize].time - self.opts.window_secs;
            while history.commits[self.tail].time <= start {
                self.count(history, self.tail, -1);
                self.tail += 1;
            }
            let edges = self.collect(history, k);
            self.edges.push(edges);
        }
    }

    fn count(&mut self, history: &History, commit: usize, delta: i32) {
        if ignored(history, commit, &self.opts) {
            return;
        }
        let files = history.touched(commit);
        let support = self.opts.min_support;
        for (i, &a) in files.iter().enumerate() {
            let n = self.files.entry(a).or_default();
            *n = n.wrapping_add_signed(delta);
            for &b in &files[i + 1..] {
                let k = key(a, b);
                let n = self.pairs.entry(k).or_default();
                *n = n.wrapping_add_signed(delta);
                match (*n, delta) {
                    (n, 1) if n == support => {
                        self.candidates.insert(k);
                    }
                    (n, -1) if n + 1 == support => {
                        self.candidates.remove(&k);
                    }
                    (0, _) => {
                        self.pairs.remove(&k);
                    }
                    _ => {}
                }
            }
        }
    }

    fn collect(&self, history: &History, k: u32) -> Vec<Edge> {
        let alive = |f: FileId| history.event_at(f, k).is_some_and(|e| e.alive());
        let mut edges: Vec<Edge> = self
            .candidates
            .iter()
            .filter_map(|&key| {
                let (a, b) = ((key >> 32) as FileId, key as FileId);
                let (count, a_commits, b_commits) = (self.pairs[&key], self.files[&a], self.files[&b]);
                let jaccard = count as f32 / (a_commits + b_commits - count) as f32;
                (jaccard >= self.opts.min_jaccard && alive(a) && alive(b)).then_some(Edge { a, b, count, jaccard, a_commits, b_commits })
            })
            .collect();
        edges.sort_unstable_by(|x, y| y.count.cmp(&x.count).then(y.jaccard.total_cmp(&x.jaccard)).then((x.a, x.b).cmp(&(y.a, y.b))));
        edges.truncate(self.opts.max_edges);
        edges
    }
}
