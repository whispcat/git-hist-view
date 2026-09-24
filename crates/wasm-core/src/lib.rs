use ghv_core::git::Kind;
use ghv_core::git::{Oid, Store, pack::Pack, proto, refs};
use ghv_core::history::{Coupling, CouplingOptions, History, LANGUAGES, Options, keyframes};
use ghv_core::layout::Rect;
use ghv_core::views::coupling::Layouts;
use ghv_core::views::deps::Deps;
use ghv_core::views::graph::{self, GraphSegment};
use ghv_core::views::treemap::{self, Cell, Layout, TreemapOptions};
use wasm_bindgen::prelude::*;

const KEYFRAMES: usize = 300;
const FRAME_CACHE: usize = 6;

#[derive(Clone, Copy, PartialEq)]
struct FrameKey {
    commit: u32,
    root: u32,
    width: f32,
    height: f32,
    window_secs: i64,
}

/// A node-link segment between two keyframes, packed as described by `views::graph::GraphSegment`.
#[wasm_bindgen(getter_with_clone)]
pub struct GraphData {
    pub ids: Vec<u32>,
    pub path: Vec<u32>,
    pub pos: Vec<f32>,
    pub size: Vec<u32>,
    pub group: Vec<u32>,
    pub dir: Vec<u8>,
    pub ends: Vec<u32>,
    pub count: Vec<u32>,
    pub weight: Vec<f32>,
    pub held: bool,
    /// Folders expanded in a dependency graph.
    pub expanded: Vec<u32>,
}

impl GraphData {
    fn new(s: GraphSegment, held: bool, expanded: Vec<u32>) -> GraphData {
        GraphData {
            ids: s.ids,
            path: s.path,
            pos: s.pos,
            size: s.size,
            group: s.group,
            dir: s.dir,
            ends: s.ends,
            count: s.count,
            weight: s.weight,
            held,
            expanded,
        }
    }
}

/// Source files whose imports still need extracting: file `i` is `bytes[offsets[i]..offsets[i + 1]]`.
#[wasm_bindgen(getter_with_clone)]
pub struct ParseBatch {
    pub names: Vec<String>,
    pub bytes: Vec<u8>,
    pub offsets: Vec<u32>,
    /// All files still missing for the request, including this batch.
    pub remaining: u32,
}

/// A treemap segment between two keyframes, in the packed layout described by `views::treemap::Segment`.
#[wasm_bindgen(getter_with_clone)]
pub struct TreemapSegment {
    pub ids: Vec<u32>,
    pub geom: Vec<f32>,
    pub loc: Vec<u32>,
    pub churn: Vec<u32>,
    pub owner: Vec<u32>,
    pub lang: Vec<u8>,
    pub flags: Vec<u8>,
    /// B repeats A because the next keyframe isn't analyzed yet.
    pub held: bool,
}

fn js(e: ghv_core::Error) -> JsError {
    JsError::new(&e.to_string())
}

#[wasm_bindgen]
#[derive(Default)]
pub struct Engine {
    store: Store,
    head: Option<Oid>,
    caps: proto::Capabilities,
    demux: Option<proto::FetchDemux>,
    history: Option<History>,
    coupling: Option<Coupling>,
    frames: Vec<(FrameKey, Vec<Cell>)>,
    layouts: Layouts,
    window_days: u32,
    deps: Deps,
    pending: Vec<ghv_core::git::Oid>,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine::default()
    }

    pub fn ls_refs_request() -> Vec<u8> {
        proto::ls_refs_request()
    }

    pub fn set_capabilities(&mut self, body: &[u8]) -> Result<(), JsError> {
        self.caps = proto::parse_capabilities(body).map_err(js)?;
        Ok(())
    }

    /// Returns the default branch name.
    pub fn set_remote_head(&mut self, ls_refs: &[u8]) -> Result<Option<String>, JsError> {
        let head = proto::parse_ls_refs(ls_refs).map_err(js)?;
        self.head = Some(head.oid);
        Ok(head.branch)
    }

    pub fn fetch_request(&mut self, depth: u32, blob_limit: u32, size_hint: usize) -> Result<Vec<u8>, JsError> {
        let head = self.head.ok_or_else(|| JsError::new("no HEAD"))?;
        self.demux = Some(proto::FetchDemux::new(size_hint));
        let opts = proto::FetchOptions { depth: Some(depth), blob_limit: Some(blob_limit) };
        Ok(proto::fetch_request(head, &self.caps, &opts))
    }

    /// Feeds a response chunk; returns server progress lines.
    pub fn fetch_push(&mut self, chunk: &[u8]) -> Result<Vec<String>, JsError> {
        let demux = self.demux.as_mut().ok_or_else(|| JsError::new("no fetch in progress"))?;
        let events = demux.push(chunk).map_err(js)?;
        Ok(events
            .into_iter()
            .filter_map(|e| match e {
                proto::Event::Progress(msg) => Some(msg),
                proto::Event::Done => None,
            })
            .collect())
    }

    pub fn fetch_finish(&mut self, progress: &js_sys::Function) -> Result<u32, JsError> {
        let demux = self.demux.take().ok_or_else(|| JsError::new("no fetch in progress"))?;
        if !demux.is_done() {
            return Err(JsError::new("pack stream ended early"));
        }
        let report = |done: u32, total: u32| {
            let _ = progress.call2(&JsValue::NULL, &done.into(), &total.into());
        };
        let pack = Pack::index(self.store.next_pack_id(), demux.pack.into_boxed_slice(), report).map_err(js)?;
        let n = pack.oids().len() as u32;
        self.store.add_pack(pack);
        Ok(n)
    }

    pub fn add_pack(&mut self, pack: Vec<u8>, idx: &[u8]) -> Result<(), JsError> {
        let pack = Pack::with_idx(self.store.next_pack_id(), pack.into_boxed_slice(), idx).map_err(js)?;
        self.store.add_pack(pack);
        Ok(())
    }

    pub fn add_loose(&mut self, hex: &str, compressed: Vec<u8>) {
        if let Some(oid) = Oid::from_hex(hex.as_bytes()) {
            self.store.add_loose(oid, compressed.into_boxed_slice());
        }
    }

    /// Accepts `HEAD` contents; returns the symbolic ref to resolve, if any.
    pub fn set_local_head(&mut self, head: &[u8]) -> Result<Option<String>, JsError> {
        match refs::parse_head(head) {
            Some(refs::HeadRef::Detached(oid)) => {
                self.head = Some(oid);
                Ok(None)
            }
            Some(refs::HeadRef::Symbolic(name)) => Ok(Some(name)),
            None => Err(JsError::new("unreadable HEAD")),
        }
    }

    pub fn resolve_ref(&mut self, name: &str, loose: Option<Vec<u8>>, packed_refs: Option<Vec<u8>>) -> Result<(), JsError> {
        let oid = loose.and_then(|l| Oid::from_hex(l.trim_ascii())).or_else(|| packed_refs.and_then(|p| refs::find_packed(&p, name)));
        self.head = Some(oid.ok_or_else(|| JsError::new(&format!("cannot resolve {name}")))?);
        Ok(())
    }

    /// Walks first-parent history; returns the number of commits that will be analyzed.
    pub fn open_history(&mut self, cap: usize) -> Result<usize, JsError> {
        let head = self.head.ok_or_else(|| JsError::new("no HEAD"))?;
        let history = History::new(&mut self.store, head, cap, Options::default()).map_err(js)?;
        self.window_days = (CouplingOptions::default().window_secs / 86_400) as u32;
        self.coupling = Some(Coupling::new(keyframes(history.commits.len(), KEYFRAMES), CouplingOptions::default()));
        let n = history.commits.len();
        self.history = Some(history);
        Ok(n)
    }

    /// Analyzes roughly `budget` lines of history; returns how many commits are done.
    pub fn step(&mut self, budget: usize) -> Result<usize, JsError> {
        let history = self.history.as_mut().ok_or_else(|| JsError::new("history not opened"))?;
        history.step(&mut self.store, budget).map_err(js)?;
        if let Some(coupling) = &mut self.coupling {
            coupling.advance(history);
        }
        Ok(history.frontier())
    }

    pub fn truncated(&self) -> bool {
        self.history().truncated
    }

    pub fn keyframes(&self) -> Vec<u32> {
        keyframes(self.history().commits.len(), KEYFRAMES)
    }

    pub fn commit_oids(&self) -> Vec<u8> {
        self.history().commits.iter().flat_map(|c| c.oid.0).collect()
    }

    pub fn commit_times(&self) -> Vec<f64> {
        self.history().commits.iter().map(|c| c.time as f64).collect()
    }

    pub fn commit_authors(&self) -> Vec<u32> {
        self.history().commits.iter().map(|c| c.author).collect()
    }

    pub fn commit_subjects(&self) -> Vec<String> {
        self.history().commits.iter().map(|c| c.subject.to_string()).collect()
    }

    pub fn author_names(&self, from: usize) -> Vec<String> {
        self.history().authors.list.iter().skip(from).map(|a| a.name.clone()).collect()
    }

    pub fn author_emails(&self, from: usize) -> Vec<String> {
        self.history().authors.list.iter().skip(from).map(|a| a.email.clone()).collect()
    }

    pub fn author_commits(&self, from: usize) -> Vec<u32> {
        self.history().authors.list.iter().skip(from).map(|a| a.commits).collect()
    }

    pub fn language_rank(&self) -> Vec<u8> {
        self.history().language_rank.clone()
    }

    /// Treemap geometry for tweening from keyframe `k` to `k + 1` (or holding still at the last keyframe).
    pub fn treemap(&mut self, k: usize, root: u32, width: f32, height: f32, window_days: u32) -> Result<TreemapSegment, JsError> {
        let frames = keyframes(self.history().commits.len(), KEYFRAMES);
        let frontier = self.history().frontier();
        let a = *frames.get(k).filter(|&&a| (a as usize) < frontier).ok_or_else(|| JsError::new("keyframe not analyzed yet"))?;
        let next = frames.get(k + 1).copied().filter(|&b| (b as usize) < frontier);
        let (b, held) = (next.unwrap_or(a), next.is_none() && k + 1 < frames.len());
        let key = |commit| FrameKey { commit, root, width, height, window_secs: i64::from(window_days) * 86_400 };
        let (fa, fb) = (self.frame(key(a)), self.frame(key(b)));
        let s = treemap::merge(&fa, &fb, &self.history().paths, Rect { x: 0.0, y: 0.0, w: width, h: height });
        Ok(TreemapSegment { ids: s.ids, geom: s.geom, loc: s.loc, churn: s.churn, owner: s.owner, lang: s.lang, flags: s.flags, held })
    }

    /// Co-change graph tweening from keyframe `k` to `k + 1`, over a window of `window_days`.
    pub fn coupling(&mut self, k: usize, window_days: u32) -> Result<GraphData, JsError> {
        if window_days != self.window_days {
            let history = self.history.as_ref().ok_or_else(|| JsError::new("history not opened"))?;
            let opts = CouplingOptions { window_secs: i64::from(window_days) * 86_400, ..CouplingOptions::default() };
            let mut c = Coupling::new(keyframes(history.commits.len(), KEYFRAMES), opts);
            c.advance(history);
            (self.coupling, self.layouts, self.window_days) = (Some(c), Layouts::default(), window_days);
        }
        let frames = keyframes(self.history().commits.len(), KEYFRAMES);
        let edges = &self.coupling.as_ref().ok_or_else(|| JsError::new("history not opened"))?.edges;
        if k >= edges.len() {
            return Err(JsError::new("keyframe not analyzed yet"));
        }
        let last = (k + 1).min(edges.len() - 1);
        let history = self.history.as_ref().unwrap();
        while self.layouts.len() <= last {
            let i = self.layouts.len();
            self.layouts.push(history, frames[i], &edges[i]);
        }
        let held = last == k && k + 1 < frames.len();
        let s = graph::merge(self.layouts.get(k).unwrap(), self.layouts.get(last).unwrap(), false);
        Ok(GraphData::new(s, held, Vec::new()))
    }

    /// Keyframes a dependency segment at `k` shows: `k`, and `k + 1` once analyzed.
    fn deps_commits(&self, k: usize) -> Result<(u32, u32, bool), JsError> {
        let frames = keyframes(self.history().commits.len(), KEYFRAMES);
        let frontier = self.history().frontier();
        let a = *frames.get(k).filter(|&&a| (a as usize) < frontier).ok_or_else(|| JsError::new("keyframe not analyzed yet"))?;
        let next = frames.get(k + 1).copied().filter(|&b| (b as usize) < frontier);
        Ok((a, next.unwrap_or(a), next.is_none() && k + 1 < frames.len()))
    }

    /// Up to `max_bytes` of source files at keyframes `k` and `k + 1` whose imports haven't been extracted.
    pub fn deps_batch(&mut self, k: usize, max_bytes: usize) -> Result<ParseBatch, JsError> {
        let (a, b, _) = self.deps_commits(k)?;
        let history = self.history.as_ref().unwrap();
        let mut missing = self.deps.missing(history, a);
        if b != a {
            missing.extend(self.deps.missing(history, b));
            missing.sort_unstable();
            missing.dedup_by_key(|(oid, _)| *oid);
        }
        let mut batch = ParseBatch { names: Vec::new(), bytes: Vec::new(), offsets: vec![0], remaining: missing.len() as u32 };
        let mut buf = Vec::new();
        self.pending.clear();
        for (oid, path) in missing {
            if batch.bytes.len() >= max_bytes {
                break;
            }
            // Blobs over the fetch size limit are absent; they simply contribute no imports.
            if self.store.read_as(&oid, Kind::Blob, &mut buf).is_ok() {
                batch.bytes.extend_from_slice(&buf);
            }
            batch.names.push(history.paths.get(path).name.to_string());
            batch.offsets.push(batch.bytes.len() as u32);
            self.pending.push(oid);
        }
        Ok(batch)
    }

    /// Records extracted imports for the last batch, in order.
    pub fn deps_insert(&mut self, records: Vec<String>) {
        for (oid, r) in std::mem::take(&mut self.pending).into_iter().zip(records) {
            self.deps.insert(oid, &r);
        }
    }

    pub fn deps_auto_expand(&mut self, k: usize) -> Result<Vec<u32>, JsError> {
        let (a, ..) = self.deps_commits(k)?;
        let history = self.history.as_ref().unwrap();
        Ok(self.deps.auto_expand(history, &mut self.store, a))
    }

    /// Folder dependency graph tweening from keyframe `k` to `k + 1`, with `expanded` folders opened.
    pub fn deps(&mut self, k: usize, expanded: Vec<u32>) -> Result<GraphData, JsError> {
        let (a, b, held) = self.deps_commits(k)?;
        let history = self.history.as_ref().unwrap();
        let fa = self.deps.frame(history, &mut self.store, a, &expanded);
        let fb = self.deps.frame(history, &mut self.store, b, &expanded);
        Ok(GraphData::new(graph::merge(&fa, &fb, true), held, expanded))
    }

    pub fn group_rank(&self) -> Vec<u32> {
        self.history().group_rank.clone()
    }

    pub fn path_count(&self) -> usize {
        self.history().paths.len()
    }

    pub fn path_names(&self, from: usize) -> Vec<String> {
        let paths = &self.history().paths;
        (from..paths.len()).map(|i| paths.get(i as u32).name.to_string()).collect()
    }

    pub fn path_parents(&self, from: usize) -> Vec<u32> {
        let paths = &self.history().paths;
        (from..paths.len()).map(|i| paths.get(i as u32).parent).collect()
    }

    pub fn languages() -> Vec<String> {
        LANGUAGES.iter().map(|(name, _)| name.to_string()).collect()
    }

    /// Lines changed per commit for commits `from..frontier`.
    pub fn churn_from(&self, from: usize) -> Vec<u32> {
        self.history().commit_churn.get(from..).unwrap_or_default().to_vec()
    }
}

impl Engine {
    fn history(&self) -> &History {
        self.history.as_ref().expect("history opened")
    }

    fn frame(&mut self, key: FrameKey) -> Vec<Cell> {
        if let Some((_, cells)) = self.frames.iter().find(|(k, _)| *k == key) {
            return cells.clone();
        }
        let opts =
            TreemapOptions { width: key.width, height: key.height, root: key.root, window_secs: key.window_secs, layout: Layout::Split };
        let cells = treemap::frame(self.history(), key.commit, &opts);
        if self.frames.len() == FRAME_CACHE {
            self.frames.remove(0);
        }
        self.frames.push((key, cells.clone()));
        cells
    }
}
