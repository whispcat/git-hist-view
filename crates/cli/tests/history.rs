mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use common::{git, repos};
use ghv_cli::repo;
use ghv_core::git::Store;
use ghv_core::history::{Coupling, CouplingOptions, Edge, History, LOCKFILES, Options, identity_key, ignored, keyframes};

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";
const EXACT: Options = Options { renames: false, merge_attribution: false, excludes: false };

fn run(path: &Path, opts: Options) -> (Store, History) {
    let (mut store, head) = repo::open(path).unwrap();
    let mut history = History::new(&mut store, head, usize::MAX, opts).unwrap();
    history.step(&mut store, usize::MAX).unwrap();
    (store, history)
}

/// Parses `--raw --numstat` output into per-commit `path -> added + removed` for regular files.
/// Binary changes ("-") count as zero, matching our model.
fn numstat(out: &str) -> Vec<(String, BTreeMap<String, u32>)> {
    let mut commits: Vec<(String, BTreeMap<String, u32>)> = Vec::new();
    let mut irregular = BTreeSet::new();
    for line in out.lines() {
        if let Some(hash) = line.strip_prefix('@') {
            commits.push((hash.to_string(), BTreeMap::new()));
            irregular.clear();
        } else if let Some(raw) = line.strip_prefix(':') {
            let (meta, path) = raw.split_once('\t').unwrap();
            let modes: Vec<&str> = meta.split(' ').take(2).collect();
            if modes.iter().any(|m| !m.starts_with("100") && *m != "000000") {
                irregular.insert(path.to_string());
            }
        } else if let [a, d, path] = line.splitn(3, '\t').collect::<Vec<_>>()[..] {
            let churn = a.parse::<u32>().unwrap_or(0) + d.parse::<u32>().unwrap_or(0);
            if !irregular.contains(path) && churn > 0 {
                commits.last_mut().unwrap().1.insert(path.to_string(), churn);
            }
        }
    }
    commits
}

#[test]
fn churn_matches_numstat() {
    for path in repos() {
        let (_, h) = run(&path, EXACT);
        let log = git(
            &path,
            &[
                "log",
                "--first-parent",
                "--reverse",
                "--no-renames",
                "--diff-algorithm=histogram",
                "--numstat",
                "--raw",
                "--no-abbrev",
                "--format=@%H",
            ],
        );
        let theirs: Vec<(String, BTreeMap<String, u32>)> = numstat(&log);
        let mut ours: Vec<BTreeMap<String, u32>> = vec![BTreeMap::new(); h.commits.len()];
        for events in &h.events {
            let mut prev = 0;
            for e in events {
                if e.churn > prev {
                    ours[e.commit as usize].insert(h.paths.full(e.path), e.churn - prev);
                }
                prev = e.churn;
            }
        }
        // Lockfiles are excluded in the product and are where the two histogram implementations diverge most.
        let is_lockfile = |p: &str| LOCKFILES.contains(&p.rsplit('/').next().unwrap());
        let theirs: Vec<_> = theirs.into_iter().map(|(h, m)| (h, m.into_iter().filter(|(p, _)| !is_lockfile(p)).collect())).collect();
        ours.iter_mut().for_each(|m| m.retain(|p, _| !is_lockfile(p)));
        let (mut total, mut mismatched) = (0usize, Vec::new());
        let sum = |m: &BTreeMap<String, u32>| m.values().map(|&n| n as u64).sum::<u64>();
        let (git_lines, our_lines): (u64, u64) = (theirs.iter().map(|t| sum(&t.1)).sum(), ours.iter().map(sum).sum());
        for (c, (hash, expected)) in theirs.iter().enumerate() {
            assert_eq!(h.commits[c].oid.to_string(), *hash);
            total += expected.len();
            for (p, n) in expected {
                if ours[c].get(p) != Some(n) {
                    mismatched.push(format!(
                        "{:>8} {hash} {p}: git {n}, ours {:?}",
                        ours[c].get(p).map_or(*n as i64, |o| *o as i64 - *n as i64).abs(),
                        ours[c].get(p)
                    ));
                }
            }
            for p in ours[c].keys().filter(|p| !expected.contains_key(*p)) {
                mismatched.push(format!("{hash} {p}: only ours"));
            }
        }
        // git's xdiff and imara-diff both implement histogram diff, but occasionally split a hunk differently.
        let rate = mismatched.len() as f64 / total.max(1) as f64;
        let drift = our_lines.abs_diff(git_lines) as f64 / git_lines.max(1) as f64;
        println!(
            "{}: {total} file changes, {} differ ({:.3}%), total churn drift {:.4}%",
            path.display(),
            mismatched.len(),
            rate * 100.0,
            drift * 100.0
        );
        mismatched.sort_unstable_by(|a, b| b.cmp(a));
        assert!(rate < 0.005 && drift < 0.001, "{}: {:#?}", path.display(), &mismatched[..mismatched.len().min(10)]);
    }
}

#[test]
fn loc_matches_head() {
    for path in repos() {
        let (_, h) = run(&path, EXACT);
        let last = h.commits.len() as u32 - 1;
        let ours: BTreeMap<String, u32> =
            h.alive_at(last).map(|(_, e)| (h.paths.full(e.path), e.loc)).filter(|(_, loc)| *loc > 0).collect();
        let diff = git(&path, &["diff", "--numstat", "--raw", "--no-renames", "--no-abbrev", EMPTY_TREE, "HEAD"]);
        let theirs = numstat(&format!("@HEAD\n{diff}")).remove(0).1;
        assert_eq!(ours, theirs, "{}", path.display());
    }
}

#[test]
fn ownership_matches_blame() {
    let opts = Options { renames: true, merge_attribution: false, excludes: false };
    let (mut all_lines, mut all_agree) = (0usize, 0usize);
    for path in repos() {
        let (_, h) = run(&path, opts);
        let last = h.commits.len() as u32 - 1;
        let mut files: Vec<_> = h.alive_at(last).filter(|(_, e)| e.loc > 0 && e.loc < 3000).collect();
        files.sort_by_key(|(_, e)| h.paths.full(e.path));
        let stride = (files.len() / 25).max(1);
        let (mut lines, mut agree) = (0usize, 0usize);
        for (file, e) in files.into_iter().step_by(stride) {
            let p = h.paths.full(e.path);
            let blame = git(&path, &["blame", "--first-parent", "--line-porcelain", "--diff-algorithm=histogram", "HEAD", "--", &p]);
            let theirs: Vec<String> = blame
                .lines()
                .scan(String::new(), |name, l| {
                    if let Some(n) = l.strip_prefix("author ") {
                        *name = n.to_string();
                    }
                    Some(l.strip_prefix("author-mail <").map(|m| identity_key(name, m.trim_end_matches('>'))))
                })
                .flatten()
                .collect();
            let ours: Vec<String> = h
                .owners(file)
                .unwrap()
                .iter()
                .flat_map(|r| {
                    let a = &h.authors.list[r.author as usize];
                    std::iter::repeat_n(identity_key(&a.name, &a.email), r.len as usize)
                })
                .collect();
            assert_eq!(ours.len(), theirs.len(), "{} {p}", path.display());
            let ok = ours.iter().zip(&theirs).filter(|(a, b)| a == b).count();
            if ok < ours.len() {
                eprintln!("  {p}: {ok}/{}", ours.len());
            }
            lines += ours.len();
            agree += ok;
        }
        let rate = agree as f64 / lines.max(1) as f64;
        println!("{}: {agree}/{lines} lines attributed like blame ({:.2}%)", path.display(), rate * 100.0);
        // Blame's rename pairing is ambiguous when many identical copies move at once (vite's playground move).
        assert!(rate > 0.95, "{}", path.display());
        (all_lines, all_agree) = (all_lines + lines, all_agree + agree);
    }
    assert!(all_agree as f64 / all_lines as f64 > 0.99);
}

#[test]
fn coupling_matches_naive_recount() {
    let opts = CouplingOptions { window_secs: 90 * 86_400, ..Default::default() };
    for path in repos() {
        let (_, h) = run(&path, Options::default());
        let frames = keyframes(h.commits.len(), 40);
        let mut coupling = Coupling::new(frames.clone(), opts);
        coupling.advance(&h);
        assert!(coupling.is_done());
        for (i, &k) in frames.iter().enumerate() {
            let start = h.commits[k as usize].time - opts.window_secs;
            let (mut files, mut pairs) = (BTreeMap::<u32, u32>::new(), BTreeMap::<(u32, u32), u32>::new());
            for c in (0..=k as usize).filter(|&c| h.commits[c].time > start) {
                if ignored(&h, c, &opts) {
                    continue;
                }
                let t = h.touched(c);
                for (j, &a) in t.iter().enumerate() {
                    *files.entry(a).or_default() += 1;
                    for &b in &t[j + 1..] {
                        *pairs.entry((a.min(b), a.max(b))).or_default() += 1;
                    }
                }
            }
            let alive = |f: u32| h.event_at(f, k).is_some_and(|e| e.alive());
            let mut naive: Vec<Edge> = pairs
                .into_iter()
                .filter(|&(_, n)| n >= opts.min_support)
                .map(|((a, b), count)| Edge {
                    a,
                    b,
                    count,
                    jaccard: count as f32 / (files[&a] + files[&b] - count) as f32,
                    a_commits: files[&a],
                    b_commits: files[&b],
                })
                .filter(|e| e.jaccard >= opts.min_jaccard && alive(e.a) && alive(e.b))
                .collect();
            naive.sort_unstable_by(|x, y| y.count.cmp(&x.count).then(y.jaccard.total_cmp(&x.jaccard)).then((x.a, x.b).cmp(&(y.a, y.b))));
            naive.truncate(opts.max_edges);
            assert_eq!(coupling.edges[i], naive, "{} keyframe {k}", path.display());
        }
    }
}

#[test]
fn streaming_matches_single_pass() {
    let path = &repos()[0];
    let (_, whole) = run(path, Options::default());
    let (mut store, head) = repo::open(path).unwrap();
    let mut streamed = History::new(&mut store, head, usize::MAX, Options::default()).unwrap();
    while !streamed.is_done() {
        streamed.step(&mut store, 5).unwrap();
    }
    assert_eq!(whole.events, streamed.events);
    assert_eq!(whole.commit_churn, streamed.commit_churn);
}

#[test]
fn coupling_ignores_bots_and_manifest_bumps() {
    let path = &repos()[0];
    let (_, h) = run(path, Options::default());
    let last = keyframes(h.commits.len(), 300);
    let edges_with = |opts: CouplingOptions| {
        let mut c = Coupling::new(last.clone(), opts);
        c.advance(&h);
        let final_edges = c.edges.last().unwrap().clone();
        final_edges
            .iter()
            .map(|e| {
                (
                    h.paths.full(h.event_at(e.a, *last.last().unwrap()).unwrap().path),
                    h.paths.full(h.event_at(e.b, *last.last().unwrap()).unwrap().path),
                    e.count,
                )
            })
            .collect::<Vec<_>>()
    };
    let filtered = edges_with(CouplingOptions::default());
    let pair = |edges: &[(String, String, u32)], a: &str| edges.iter().find(|(x, y, _)| x == a || y == a).cloned();
    assert_eq!(filtered.len(), 1, "{filtered:?}");
    assert_eq!(pair(&filtered, "src/gen.txt").unwrap().2, 30);
    let raw = edges_with(CouplingOptions { ignore_bots: false, ignore_manifest_only: false, ..Default::default() });
    assert_eq!(pair(&raw, "src/gen.txt").unwrap().2, 31);
    assert!(pair(&raw, "package.json").is_some(), "{raw:?}");
}
