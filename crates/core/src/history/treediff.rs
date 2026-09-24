use std::cmp::Ordering;

use super::paths::{PathId, Paths};
use crate::Result;
use crate::git::{EntryKind, Kind, Oid, Store, TreeEntry, TreeIter};

/// A changed regular file. Symlinks and submodules are not files for our purposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Change {
    pub path: PathId,
    pub old: Option<Oid>,
    pub new: Option<Oid>,
}

/// Git sorts tree entries as if directory names had a trailing `/`.
fn git_order(a: &TreeEntry, b: &TreeEntry) -> Ordering {
    let suffix = |e: &TreeEntry| if e.kind == EntryKind::Tree { b'/' } else { 0 };
    let n = a.name.len().min(b.name.len());
    a.name[..n].cmp(&b.name[..n]).then_with(|| {
        let ca = a.name.get(n).copied().unwrap_or_else(|| suffix(a));
        let cb = b.name.get(n).copied().unwrap_or_else(|| suffix(b));
        ca.cmp(&cb)
    })
}

fn read_tree<'a>(store: &mut Store, tree: Option<Oid>, buf: &'a mut Vec<u8>) -> Result<Vec<TreeEntry<'a>>> {
    buf.clear();
    if let Some(oid) = tree {
        store.read_as(&oid, Kind::Tree, buf)?;
    }
    TreeIter::new(buf).filter(|e| e.as_ref().map_or(true, |e| matches!(e.kind, EntryKind::Tree | EntryKind::Blob))).collect()
}

/// Appends file-level changes between two trees, skipping identical subtrees without reading them.
pub fn diff_trees(
    store: &mut Store,
    paths: &mut Paths,
    parent: PathId,
    old: Option<Oid>,
    new: Option<Oid>,
    out: &mut Vec<Change>,
) -> Result<()> {
    let (mut old_buf, mut new_buf) = (Vec::new(), Vec::new());
    let a = read_tree(store, old, &mut old_buf)?;
    let b = read_tree(store, new, &mut new_buf)?;
    let (mut i, mut j) = (0, 0);
    while i < a.len() || j < b.len() {
        let order = match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) => git_order(x, y),
            (Some(_), None) => Ordering::Less,
            _ => Ordering::Greater,
        };
        let ea = (order != Ordering::Greater).then(|| a[i]);
        let eb = (order != Ordering::Less).then(|| b[j]);
        i += usize::from(ea.is_some());
        j += usize::from(eb.is_some());
        if ea.map(|x| x.oid) == eb.map(|y| y.oid) {
            continue;
        }
        let e = ea.or(eb).unwrap();
        let is_dir = e.kind == EntryKind::Tree;
        let path = paths.child(parent, e.name, is_dir);
        if is_dir {
            diff_trees(store, paths, path, ea.map(|x| x.oid), eb.map(|y| y.oid), out)?;
        } else {
            out.push(Change { path, old: ea.map(|x| x.oid), new: eb.map(|y| y.oid) });
        }
    }
    Ok(())
}
