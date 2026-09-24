mod common;

use std::collections::BTreeSet;

use common::{git, repos};
use ghv_cli::repo;
use ghv_core::git::Oid;
use ghv_core::history::{Authors, Paths, ROOT, diff_trees, first_parent};

#[test]
fn walk_matches_rev_list() {
    for path in repos() {
        let (mut store, head) = repo::open(&path).unwrap();
        let commits = first_parent(&mut store, head, usize::MAX, &mut Authors::default()).unwrap().commits;
        let ours: Vec<String> = commits.iter().map(|c| c.oid.to_string()).collect();
        let theirs: Vec<String> = git(&path, &["rev-list", "--first-parent", "--reverse", "HEAD"]).lines().map(String::from).collect();
        assert_eq!(ours, theirs, "{}", path.display());
    }
}

type FileChange = (String, Option<String>, Option<String>);

/// Parses `git diff-tree --raw` into regular-file changes; symlinks and gitlinks count as absent.
fn git_changes(raw: &str) -> BTreeSet<FileChange> {
    let file = |mode: &str, oid: &str| (mode.starts_with("100")).then(|| oid.to_string());
    raw.lines()
        .filter_map(|l| {
            let (meta, path) = l.strip_prefix(':')?.split_once('\t')?;
            let m: Vec<&str> = meta.split(' ').collect();
            let (old, new) = (file(m[0], m[2]), file(m[1], m[3]));
            (old != new).then(|| (path.to_string(), old, new))
        })
        .collect()
}

#[test]
fn tree_diff_matches_diff_tree() {
    for path in repos() {
        let (mut store, head) = repo::open(&path).unwrap();
        let commits = first_parent(&mut store, head, usize::MAX, &mut Authors::default()).unwrap().commits;
        let mut paths = Paths::default();
        let mut prev: Option<(Oid, Oid)> = None;
        for c in &commits {
            let mut out = Vec::new();
            diff_trees(&mut store, &mut paths, ROOT, prev.map(|p| p.1), Some(c.tree), &mut out).unwrap();
            let ours: BTreeSet<FileChange> =
                out.iter().map(|ch| (paths.full(ch.path), ch.old.map(|o| o.to_string()), ch.new.map(|o| o.to_string()))).collect();
            let from = prev.map_or("--root".to_string(), |p| p.0.to_string());
            let mut args = vec!["diff-tree", "-r", "--no-renames", "--no-abbrev", "--raw"];
            args.extend(if prev.is_some() { vec![from.as_str()] } else { vec!["--root"] });
            let to = c.oid.to_string();
            args.push(&to);
            assert_eq!(ours, git_changes(&git(&path, &args)), "{} at {}", path.display(), c.oid);
            prev = Some((c.oid, c.tree));
        }
    }
}
