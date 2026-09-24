use super::Authors;
use crate::Result;
use crate::git::{Commit, Kind, Oid, Store};

#[derive(Debug, Clone)]
pub struct CommitMeta {
    pub oid: Oid,
    pub tree: Oid,
    pub side_parents: Vec<Oid>,
    pub author: u32,
    pub time: i64,
    pub subject: Box<str>,
}

pub struct Walk {
    pub commits: Vec<CommitMeta>,
    /// Older history exists but was cut off by the cap or a shallow fetch.
    pub truncated: bool,
}

/// Walks first-parent history from `head` (up to `cap` commits or a shallow boundary), returned oldest first.
pub fn first_parent(store: &mut Store, head: Oid, cap: usize, authors: &mut Authors) -> Result<Walk> {
    let (mut out, mut buf, mut next) = (Vec::new(), Vec::new(), Some(head));
    let mut max_time = i64::MIN;
    while let Some(oid) = next.filter(|o| out.len() < cap && store.contains(o)) {
        store.read_as(&oid, Kind::Commit, &mut buf)?;
        let c = Commit::parse(&buf)?;
        next = c.parents.first().copied();
        out.push(CommitMeta {
            oid,
            tree: c.tree,
            side_parents: c.parents.get(1..).unwrap_or_default().to_vec(),
            author: authors.intern(c.author.name, c.author.email),
            time: c.committer_time,
            subject: String::from_utf8_lossy(c.subject()).into(),
        });
    }
    let truncated = next.is_some();
    out.reverse();
    // Committer clocks drift and rebases reorder; a running max keeps time windows monotonic.
    for c in &mut out {
        max_time = max_time.max(c.time);
        c.time = max_time;
    }
    Ok(Walk { commits: out, truncated })
}

/// Up to `max` commit indices spread evenly over `n` commits, always including the first and last.
pub fn keyframes(n: usize, max: usize) -> Vec<u32> {
    match n {
        0 => Vec::new(),
        _ if n <= max => (0..n as u32).collect(),
        _ => {
            let mut k: Vec<u32> = (0..max).map(|i| ((i * (n - 1) + (max - 1) / 2) / (max - 1)) as u32).collect();
            k.dedup();
            k
        }
    }
}
