use rustc_hash::{FxHashMap, FxHashSet};

use super::authors::{Authors, Mailmap};
use super::filters::{Excludes, LANGUAGES, language};
use super::lines::{Differ, hash_lines, is_binary};
use super::owners::{Run, dominant, splice};
use super::paths::{PathId, Paths, ROOT};
use super::treediff::{Change, diff_trees};
use super::walk::{CommitMeta, first_parent};
use crate::git::{Commit, Kind, Oid, Store, TreeIter};
use crate::{Error, Result};

pub type FileId = u32;
pub const DEAD: u32 = u32::MAX;
const SIDE_BRANCH_LIMIT: usize = 200;
const RENAME_PAIR_LIMIT: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub struct Options {
    pub renames: bool,
    pub merge_attribution: bool,
    pub excludes: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { renames: true, merge_attribution: true, excludes: true }
    }
}

/// A file's state right after a mainline commit touched it. `churn` is cumulative so window sums are a subtraction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub commit: u32,
    pub path: PathId,
    pub loc: u32,
    pub owner: u32,
    pub churn: u32,
    /// Content at this point, for re-reading source (e.g. to parse imports); zero on deletion.
    pub blob: Oid,
}

impl Event {
    pub fn alive(&self) -> bool {
        self.loc != DEAD
    }
}

struct Live {
    path: PathId,
    lines: Vec<u32>,
    binary: bool,
    owners: Vec<Run>,
    churn: u32,
}

struct Blob {
    oid: Oid,
    lines: Vec<u32>,
    binary: bool,
}

enum Op {
    Add(PathId, Blob),
    Modify(PathId, Blob),
    Delete(PathId),
    Rename(PathId, PathId, Blob),
}

/// One forward pass over first-parent history that yields LOC, churn, ownership and co-change data.
pub struct History {
    pub commits: Vec<CommitMeta>,
    pub authors: Authors,
    pub paths: Paths,
    pub events: Vec<Vec<Event>>,
    pub commit_churn: Vec<u32>,
    pub truncated: bool,
    /// Language ids ordered by file count in HEAD's tree, so color slots stay fixed while history streams in.
    pub language_rank: Vec<u8>,
    /// Top-level entries (directories, or ROOT for loose root files) ordered by file count in HEAD's tree.
    pub group_rank: Vec<PathId>,
    touched_start: Vec<u32>,
    touched: Vec<FileId>,
    live: Vec<Option<Live>>,
    alive: FxHashMap<PathId, FileId>,
    excludes: Excludes,
    opts: Options,
    mainline: FxHashSet<Oid>,
    prev_tree: Option<Oid>,
    differ: Differ,
    buf: Vec<u8>,
    runs: Vec<Run>,
    tally: FxHashMap<u32, u32>,
}

fn read_mailmap(store: &mut Store, head: Oid) -> Result<Mailmap> {
    let mut buf = Vec::new();
    store.read_as(&head, Kind::Commit, &mut buf)?;
    let tree = Commit::parse(&buf)?.tree;
    store.read_as(&tree, Kind::Tree, &mut buf)?;
    let entry = TreeIter::new(&buf).filter_map(Result::ok).find(|e| e.name == b".mailmap").map(|e| e.oid);
    Ok(match entry {
        Some(oid) if store.read_as(&oid, Kind::Blob, &mut buf).is_ok() => Mailmap::parse(&buf),
        _ => Mailmap::default(),
    })
}

impl History {
    pub fn new(store: &mut Store, head: Oid, cap: usize, opts: Options) -> Result<Self> {
        let mut authors = Authors::with_mailmap(read_mailmap(store, head)?);
        let walk = first_parent(store, head, cap, &mut authors)?;
        let commits = walk.commits;
        let mut paths = Paths::default();
        let mut excludes = Excludes::new(opts.excludes);
        let (language_rank, group_rank) = match commits.last() {
            Some(c) => rank_head(store, &mut paths, &mut excludes, c.tree)?,
            None => Default::default(),
        };
        Ok(History {
            truncated: walk.truncated,
            language_rank,
            group_rank,
            mainline: commits.iter().map(|c| c.oid).collect(),
            commit_churn: Vec::with_capacity(commits.len()),
            touched_start: vec![0],
            commits,
            authors,
            paths,
            events: Vec::new(),
            touched: Vec::new(),
            live: Vec::new(),
            alive: FxHashMap::default(),
            excludes,
            opts,
            prev_tree: None,
            differ: Differ::default(),
            buf: Vec::new(),
            runs: Vec::new(),
            tally: FxHashMap::default(),
        })
    }

    /// Number of commits processed so far; snapshots are valid for commits below this.
    pub fn frontier(&self) -> usize {
        self.commit_churn.len()
    }

    pub fn is_done(&self) -> bool {
        self.frontier() == self.commits.len()
    }

    /// Processes commits until roughly `budget` lines have been hashed or diffed.
    pub fn step(&mut self, store: &mut Store, budget: usize) -> Result<()> {
        let mut work = 0;
        while work < budget && !self.is_done() {
            work += self.process(store)? + 1;
        }
        Ok(())
    }

    pub fn touched(&self, commit: usize) -> &[FileId] {
        &self.touched[self.touched_start[commit] as usize..self.touched_start[commit + 1] as usize]
    }

    pub fn event_at(&self, file: FileId, commit: u32) -> Option<&Event> {
        let events = &self.events[file as usize];
        events.partition_point(|e| e.commit <= commit).checked_sub(1).map(|i| &events[i])
    }

    pub fn alive_at(&self, commit: u32) -> impl Iterator<Item = (FileId, &Event)> {
        (0..self.events.len() as FileId).filter_map(move |f| self.event_at(f, commit).filter(|e| e.alive()).map(|e| (f, e)))
    }

    /// Lines changed in `file` by commits in `(after, upto]`.
    pub fn churn_between(&self, file: FileId, after: Option<u32>, upto: u32) -> u32 {
        let cum = |c: Option<u32>| c.and_then(|c| self.event_at(file, c)).map_or(0, |e| e.churn);
        cum(Some(upto)) - cum(after)
    }

    /// Current line ownership of a live file at the frontier.
    pub fn owners(&self, file: FileId) -> Option<&[Run]> {
        self.live.get(file as usize)?.as_ref().map(|l| &*l.owners)
    }

    fn load(&mut self, store: &mut Store, oid: Oid) -> Result<Blob> {
        let binary = match store.read(&oid, &mut self.buf) {
            // Blobs over the fetch size limit are omitted by the server; treat them like binaries.
            Err(Error::Missing(_)) => true,
            Err(e) => return Err(e),
            Ok(_) => is_binary(&self.buf),
        };
        let mut lines = Vec::new();
        if !binary {
            hash_lines(&self.buf, &mut lines);
        }
        Ok(Blob { oid, lines, binary })
    }

    fn process(&mut self, store: &mut Store) -> Result<usize> {
        let c = self.frontier();
        let tree = self.commits[c].tree;
        let mut changes = Vec::new();
        diff_trees(store, &mut self.paths, ROOT, self.prev_tree, Some(tree), &mut changes)?;
        self.prev_tree = Some(tree);
        changes.retain(|ch| !self.excludes.is_excluded(&self.paths, ch.path));

        let side = if self.opts.merge_attribution && !self.commits[c].side_parents.is_empty() {
            self.side_authors(store, c)?
        } else {
            FxHashMap::default()
        };
        let ops = self.plan(store, changes)?;
        let mut work = 0;
        let mut churn = 0;
        for op in ops {
            let (file, delta, lines) = self.apply(op, c, &side);
            work += lines;
            churn += delta;
            self.touched.push(file);
        }
        self.touched_start.push(self.touched.len() as u32);
        self.commit_churn.push(churn);
        Ok(work)
    }

    /// Turns raw changes into operations, pairing deletions with additions as renames.
    fn plan(&mut self, store: &mut Store, changes: Vec<Change>) -> Result<Vec<Op>> {
        let mut deleted: Vec<(PathId, Oid)> = Vec::new();
        let mut added: Vec<(PathId, Oid)> = Vec::new();
        let mut ops = Vec::with_capacity(changes.len());
        for ch in changes {
            match (ch.old, ch.new) {
                (Some(old), None) => deleted.push((ch.path, old)),
                (None, Some(new)) => added.push((ch.path, new)),
                (Some(_), Some(new)) if self.alive.contains_key(&ch.path) => ops.push(Op::Modify(ch.path, self.load(store, new)?)),
                (_, Some(new)) => ops.push(Op::Add(ch.path, self.load(store, new)?)),
                (None, None) => {}
            }
        }
        deleted.retain(|d| self.alive.contains_key(&d.0));
        let mut added_blobs = Vec::with_capacity(added.len());
        for &(path, oid) in &added {
            added_blobs.push(Some((path, oid, self.load(store, oid)?)));
        }
        if self.opts.renames {
            self.pair_renames(&mut deleted, &mut added_blobs, &mut ops);
        }
        ops.extend(deleted.into_iter().map(|(path, _)| Op::Delete(path)));
        ops.extend(added_blobs.into_iter().flatten().map(|(path, _, blob)| Op::Add(path, blob)));
        Ok(ops)
    }

    fn pair_renames(&self, deleted: &mut Vec<(PathId, Oid)>, added: &mut [Option<(PathId, Oid, Blob)>], ops: &mut Vec<Op>) {
        let rename = |ops: &mut Vec<Op>, del: PathId, add: &mut Option<(PathId, Oid, Blob)>| {
            let (to, _, blob) = add.take().unwrap();
            ops.push(Op::Rename(del, to, blob));
        };
        for add in added.iter_mut() {
            let oid = add.as_ref().unwrap().1;
            if let Some(i) = deleted.iter().position(|d| d.1 == oid) {
                rename(ops, deleted.swap_remove(i).0, add);
            }
        }
        // Edited moves, like git -M: pair greedily, preferring paths that agree on more trailing
        // components (directory moves), then line similarity (>= 50%). Huge commits only compare
        // files with the same name to stay linear.
        let exhaustive = deleted.len() * added.len() <= RENAME_PAIR_LIMIT;
        let old: Vec<&[u32]> =
            deleted.iter().map(|d| self.live[self.alive[&d.0] as usize].as_ref().map_or(&[][..], |l| &l.lines)).collect();
        let mut scored = Vec::new();
        for (a, add) in added.iter().enumerate() {
            let Some((to, _, blob)) = add else { continue };
            for (d, &(from, _)) in deleted.iter().enumerate() {
                let suffix = self.paths.common_suffix(from, *to);
                if exhaustive || suffix > 0 {
                    let score = similarity(old[d], &blob.lines);
                    if score >= 0.5 {
                        scored.push((suffix, score, d, a));
                    }
                }
            }
        }
        scored.sort_unstable_by(|x, y| y.0.cmp(&x.0).then(y.1.total_cmp(&x.1)).then((x.2, x.3).cmp(&(y.2, y.3))));
        let mut used = vec![false; deleted.len()];
        for (_, _, d, a) in scored {
            if !used[d] && added[a].is_some() {
                used[d] = true;
                rename(ops, deleted[d].0, &mut added[a]);
            }
        }
        let mut i = 0;
        deleted.retain(|_| (!used[i], i += 1).0);
    }

    /// Applies one operation; returns (file, churn, lines processed).
    fn apply(&mut self, op: Op, c: usize, side: &FxHashMap<PathId, u32>) -> (FileId, u32, usize) {
        let author = |path: PathId| side.get(&path).copied().unwrap_or(self.commits[c].author);
        let (file, path, blob) = match op {
            Op::Delete(path) => {
                let file = self.alive.remove(&path).expect("deleted file was alive");
                let live = self.live[file as usize].take().unwrap();
                let removed = if live.binary { 0 } else { live.lines.len() as u32 };
                let churn = live.churn + removed;
                let owner = self.events[file as usize].last().map_or(0, |e| e.owner);
                self.events[file as usize].push(Event { commit: c as u32, path, loc: DEAD, owner, churn, blob: Oid::default() });
                return (file, removed, 0);
            }
            Op::Add(path, blob) => {
                let file = self.live.len() as FileId;
                self.live.push(Some(Live { path, lines: Vec::new(), binary: true, owners: Vec::new(), churn: 0 }));
                self.events.push(Vec::new());
                self.alive.insert(path, file);
                (file, path, blob)
            }
            Op::Modify(path, blob) => (self.alive[&path], path, blob),
            Op::Rename(from, to, blob) => {
                let file = self.alive.remove(&from).expect("renamed file was alive");
                self.alive.insert(to, file);
                (file, to, blob)
            }
        };
        let author = author(path);
        let live = self.live[file as usize].as_mut().unwrap();
        self.runs.clear();
        // git's numstat reports any change involving a binary side as "-", i.e. no line churn.
        let delta = match (live.binary, blob.binary) {
            (_, true) => 0,
            (true, false) => {
                let len = blob.lines.len() as u32;
                if len > 0 {
                    self.runs.push(Run { author, len });
                }
                len
            }
            (false, false) => {
                let hunks: Vec<_> = self.differ.diff(&live.lines, &blob.lines).collect();
                let delta = hunks.iter().map(|h| h.before.len() + h.after.len()).sum::<usize>() as u32;
                splice(&live.owners, hunks.into_iter(), author, &mut self.runs);
                delta
            }
        };
        let processed = live.lines.len() + blob.lines.len();
        let oid = blob.oid;
        live.path = path;
        live.churn += delta;
        live.binary = blob.binary;
        live.lines = blob.lines;
        std::mem::swap(&mut live.owners, &mut self.runs);
        let owner = dominant(&live.owners, &mut self.tally).unwrap_or(author);
        let loc = live.lines.len() as u32;
        let churn = live.churn;
        self.events[file as usize].push(Event { commit: c as u32, path, loc, owner, churn, blob: oid });
        (file, delta, processed)
    }

    /// For a merge, attributes each path to the newest side-branch commit that touched it.
    fn side_authors(&mut self, store: &mut Store, c: usize) -> Result<FxHashMap<PathId, u32>> {
        let mut out = FxHashMap::default();
        let (mut buf, mut changes) = (Vec::new(), Vec::new());
        for &start in &self.commits[c].side_parents.clone() {
            let mut next = Some(start);
            let mut steps = 0;
            while let Some(oid) = next.filter(|o| steps < SIDE_BRANCH_LIMIT && !self.mainline.contains(o) && store.contains(o)) {
                store.read_as(&oid, Kind::Commit, &mut buf)?;
                let commit = Commit::parse(&buf)?;
                let author = self.authors.lookup(commit.author.name, commit.author.email);
                let (tree, parent) = (commit.tree, commit.parents.first().copied());
                let parent_tree = match parent.filter(|p| store.contains(p)) {
                    Some(p) => {
                        store.read_as(&p, Kind::Commit, &mut buf)?;
                        Some(Commit::parse(&buf)?.tree)
                    }
                    None => None,
                };
                changes.clear();
                diff_trees(store, &mut self.paths, ROOT, parent_tree, Some(tree), &mut changes)?;
                for ch in &changes {
                    out.entry(ch.path).or_insert(author);
                }
                next = parent;
                steps += 1;
            }
        }
        Ok(out)
    }
}

/// Ranks languages and top-level groups by file count at HEAD, fixing color slots before history streams in.
fn rank_head(store: &mut Store, paths: &mut Paths, excludes: &mut Excludes, tree: Oid) -> Result<(Vec<u8>, Vec<PathId>)> {
    let mut files = Vec::new();
    diff_trees(store, paths, ROOT, None, Some(tree), &mut files)?;
    let mut languages = vec![0u32; LANGUAGES.len()];
    let mut groups: FxHashMap<PathId, u32> = FxHashMap::default();
    for f in files.iter().filter(|f| !excludes.is_excluded(paths, f.path)) {
        languages[usize::from(language(&paths.get(f.path).name))] += 1;
        *groups.entry(top_level(paths, f.path)).or_default() += 1;
    }
    let mut language_rank: Vec<u8> = (1..LANGUAGES.len() as u8).filter(|&l| languages[usize::from(l)] > 0).collect();
    language_rank.sort_by_key(|&l| std::cmp::Reverse(languages[usize::from(l)]));
    let mut group_rank: Vec<PathId> = groups.keys().copied().collect();
    group_rank.sort_by_key(|g| (std::cmp::Reverse(groups[g]), *g));
    Ok((language_rank, group_rank))
}

/// The top-level directory containing `path`, or ROOT for files at the repository root.
pub fn top_level(paths: &Paths, mut path: PathId) -> PathId {
    loop {
        let parent = paths.get(path).parent;
        if parent == ROOT {
            return if paths.get(path).is_dir { path } else { ROOT };
        }
        path = parent;
    }
}

/// Fraction of lines (as a multiset) shared by both versions, relative to the longer one.
fn similarity(a: &[u32], b: &[u32]) -> f32 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let mut counts: FxHashMap<u32, i32> = FxHashMap::default();
    for &l in a {
        *counts.entry(l).or_default() += 1;
    }
    let shared = b.iter().filter(|l| counts.get_mut(l).is_some_and(|n| (*n > 0).then(|| *n -= 1).is_some())).count();
    shared as f32 / a.len().max(b.len()) as f32
}
