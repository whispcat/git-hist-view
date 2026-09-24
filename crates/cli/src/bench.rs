use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::Instant,
};

use ghv_cli::repo;
use ghv_core::deps::{Import, parse_records};
use ghv_core::git::{Kind, Store};
use ghv_core::history::{Coupling, CouplingOptions, History, Options, ROOT, keyframes};
use ghv_core::layout::Rect;
use ghv_core::views::coupling::Layouts;
use ghv_core::views::deps::Deps;
use ghv_core::views::graph::merge;
use ghv_core::views::treemap::{self, Layout, TreemapOptions};
use ghv_parse::{Extractor, Lang};

use crate::CliResult;

const KEYFRAMES: usize = 300;

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

/// Loads a repository and runs the whole history pass, as the browser does before any view.
fn analyze(path: &Path) -> Result<(Store, History), Box<dyn std::error::Error>> {
    let (mut store, head) = repo::open(path)?;
    let mut history = History::new(&mut store, head, usize::MAX, Options::default())?;
    history.step(&mut store, usize::MAX)?;
    Ok((store, history))
}

/// Mean distance moved by items present in consecutive frames.
#[derive(Default)]
struct Drift {
    total: f64,
    count: f64,
}

impl Drift {
    fn add(&mut self, distance: f32) {
        self.total += f64::from(distance);
        self.count += 1.0;
    }

    fn mean(&self) -> f64 {
        self.total / self.count.max(1.0)
    }
}

/// Runs the full analysis the browser runs and reports timings and headline numbers.
pub fn stats(path: &Path) -> CliResult {
    let t = Instant::now();
    let (mut store, head) = repo::open(path)?;
    let mut history = History::new(&mut store, head, usize::MAX, Options::default())?;
    let walked = ms(t);
    history.step(&mut store, usize::MAX)?;
    let analyzed = ms(t);
    let frames = keyframes(history.commits.len(), KEYFRAMES);
    let mut coupling = Coupling::new(frames.clone(), CouplingOptions::default());
    coupling.advance(&history);
    let coupled = ms(t);

    let last = history.commits.len() as u32 - 1;
    let alive: Vec<_> = history.alive_at(last).collect();
    let loc: u64 = alive.iter().map(|(_, e)| u64::from(e.loc)).sum();
    let churn: u64 = history.commit_churn.iter().map(|&c| u64::from(c)).sum();
    println!(
        "{} commits, {} authors, {} files ({loc} lines) at HEAD, {churn} lines churned",
        history.commits.len(),
        history.authors.list.len(),
        alive.len()
    );
    println!(
        "load and walk {walked:.0}ms, history {:.0}ms, coupling over {} keyframes {:.0}ms, total {coupled:.0}ms",
        analyzed - walked,
        frames.len(),
        coupled - analyzed
    );
    let name = |f| history.paths.full(history.event_at(f, last).expect("coupled files are alive").path);
    for e in coupling.edges.last().into_iter().flatten().take(5) {
        println!("  {} <-> {} ({}x, jaccard {:.2})", name(e.a), name(e.b), e.count, e.jaccard);
    }
    Ok(())
}

/// Times treemap frames at every keyframe and compares layouts by how far files move between keyframes.
pub fn treemap(path: &Path) -> CliResult {
    let (_, history) = analyze(path)?;
    let bounds = Rect { x: 0.0, y: 0.0, w: 1400.0, h: 800.0 };
    for layout in [Layout::Split, Layout::Squarify] {
        let opts = TreemapOptions { width: bounds.w, height: bounds.h, root: ROOT, window_secs: 90 * 86_400, layout };
        let (mut worst, mut total, mut cells, mut aspect, mut drift) = (0f64, 0f64, 0, 0f64, Drift::default());
        let mut prev: Option<Vec<treemap::Cell>> = None;
        for &k in &keyframes(history.commits.len(), KEYFRAMES) {
            let t = Instant::now();
            let frame = treemap::frame(&history, k, &opts);
            if let Some(prev) = &prev {
                treemap::merge(prev, &frame, &history.paths, bounds);
                let before: HashMap<u32, Rect> = prev.iter().filter(|c| !c.is_dir).map(|c| (c.id, c.rect)).collect();
                for c in frame.iter().filter(|c| !c.is_dir) {
                    if let Some(r) = before.get(&c.id) {
                        drift.add((r.x - c.rect.x).abs() + (r.y - c.rect.y).abs());
                    }
                }
            }
            (worst, total, cells) = (worst.max(ms(t)), total + ms(t), cells.max(frame.len()));
            let files: Vec<Rect> = frame.iter().filter(|c| !c.is_dir && c.rect.w > 1.0 && c.rect.h > 1.0).map(|c| c.rect).collect();
            aspect += files.iter().map(|r| f64::from(r.w.max(r.h) / r.w.min(r.h))).sum::<f64>() / files.len().max(1) as f64;
            prev = Some(frame);
        }
        let n = keyframes(history.commits.len(), KEYFRAMES).len() as f64;
        println!(
            "{layout:?}: up to {cells} cells, frame and merge {:.2}ms mean, {worst:.2}ms worst; files move {:.1}px per keyframe, mean aspect {:.2}",
            total / n,
            drift.mean(),
            aspect / n
        );
    }
    Ok(())
}

/// Lays out the coupling graph at every keyframe in order and reports cost and node drift.
pub fn coupling(path: &Path) -> CliResult {
    let (_, history) = analyze(path)?;
    let frames = keyframes(history.commits.len(), KEYFRAMES);
    let mut coupling = Coupling::new(frames.clone(), CouplingOptions::default());
    coupling.advance(&history);
    let mut layouts = Layouts::default();
    let (mut total, mut worst, mut nodes, mut drift) = (0f64, 0f64, 0, Drift::default());
    for (k, edges) in coupling.edges.iter().enumerate() {
        let t = Instant::now();
        layouts.push(&history, frames[k], edges);
        (total, worst) = (total + ms(t), worst.max(ms(t)));
        let frame = layouts.get(k).expect("just pushed");
        nodes = nodes.max(frame.nodes.len());
        if let Some(prev) = k.checked_sub(1).and_then(|p| layouts.get(p)) {
            let before: HashMap<u32, [f32; 2]> = prev.nodes.iter().copied().zip(prev.pos.iter().copied()).collect();
            for (id, p) in frame.nodes.iter().zip(&frame.pos) {
                if let Some(q) = before.get(id) {
                    drift.add((p[0] - q[0]).hypot(p[1] - q[1]));
                }
            }
        }
    }
    let t = Instant::now();
    for k in 1..layouts.len() {
        merge(layouts.get(k - 1).expect("in range"), layouts.get(k).expect("in range"), false);
    }
    let merge_ms = ms(t) / frames.len() as f64;
    let edges = coupling.edges.iter().map(Vec::len).max().unwrap_or(0);
    println!(
        "{} keyframes, up to {nodes} nodes and {edges} edges: layout {total:.0}ms total, {worst:.1}ms worst, merge {merge_ms:.2}ms; nodes move {:.1} units per keyframe",
        frames.len(),
        drift.mean()
    );
    Ok(())
}

/// Parses and resolves imports at HEAD, listing relative JS imports that resolved to nothing (usually assets).
pub fn deps(path: &Path) -> CliResult {
    let (mut store, history) = analyze(path)?;
    let commit = history.commits.len() as u32 - 1;
    let mut deps = Deps::default();
    let (t, mut extractor, mut buf, mut records) = (Instant::now(), Extractor::default(), Vec::new(), Vec::new());
    let missing = deps.missing(&history, commit);
    for &(oid, path) in &missing {
        store.read_as(&oid, Kind::Blob, &mut buf)?;
        let text = Lang::from_name(&history.paths.get(path).name).map(|l| extractor.extract(l, &buf).join("\n")).unwrap_or_default();
        deps.insert(oid, &text);
        records.push((history.paths.full(path), text));
    }
    let parsed = ms(t);
    let resolved = deps.resolved(&history, &mut store, commit);
    let expanded = deps.auto_expand(&history, &mut store, commit);
    let frame = deps.frame(&history, &mut store, commit, &expanded);
    println!(
        "{} files parsed in {parsed:.0}ms, {} imports resolved in {:.0}ms; {} folders opened: {} nodes, {} edges",
        missing.len(),
        resolved.edges.len(),
        ms(t) - parsed,
        expanded.len(),
        frame.nodes.len(),
        frame.edges.len()
    );

    let importers: HashSet<String> = resolved.edges.iter().map(|&(a, _)| history.paths.full(a)).collect();
    let unresolved: Vec<String> = records
        .iter()
        .filter(|(file, _)| !importers.contains(file))
        .flat_map(|(file, text)| {
            parse_records(text).into_iter().filter_map(move |i| match i {
                Import::Js(spec) if spec.starts_with('.') => Some(format!("{file}: {spec}")),
                _ => None,
            })
        })
        .collect();
    println!("relative JS imports in files with no resolved import: {}", unresolved.len());
    for u in unresolved.iter().take(8) {
        println!("  {u}");
    }
    let name = |i: u32| history.paths.full(i) + if history.paths.get(i).is_dir { "/" } else { "" };
    let mut edges: Vec<_> = frame.edges.iter().collect();
    edges.sort_by_key(|e| std::cmp::Reverse(e.count));
    for e in edges.iter().take(6) {
        println!("  {} -> {} ({} imports)", name(e.a), name(e.b), e.count);
    }
    Ok(())
}
