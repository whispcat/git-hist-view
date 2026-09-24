use imara_diff::Hunk;
use rustc_hash::FxHashMap;

/// A run of consecutive lines last written by one author.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Run {
    pub author: u32,
    pub len: u32,
}

fn push(out: &mut Vec<Run>, author: u32, len: u32) {
    match out.last_mut() {
        _ if len == 0 => {}
        Some(last) if last.author == author => last.len += len,
        _ => out.push(Run { author, len }),
    }
}

/// Rewrites line ownership after a diff: untouched lines keep their owner, inserted lines go to `author`.
pub fn splice(old: &[Run], hunks: impl Iterator<Item = Hunk>, author: u32, out: &mut Vec<Run>) {
    out.clear();
    let mut runs = old.iter().copied();
    let mut current = runs.next();
    let mut advance = |mut n: u32, out: &mut Vec<Run>, keep: bool| {
        while n > 0 {
            let Some(run) = current.as_mut() else { return };
            let take = n.min(run.len);
            if keep {
                push(out, run.author, take);
            }
            run.len -= take;
            n -= take;
            if run.len == 0 {
                current = runs.next();
            }
        }
    };
    let mut pos = 0;
    for h in hunks {
        advance(h.before.start - pos, out, true);
        advance(h.before.len() as u32, out, false);
        push(out, author, h.after.len() as u32);
        pos = h.before.end;
    }
    advance(u32::MAX, out, true);
}

/// The author owning the most lines, with ties going to the lower author id for determinism.
pub fn dominant(runs: &[Run], scratch: &mut FxHashMap<u32, u32>) -> Option<u32> {
    scratch.clear();
    for r in runs {
        *scratch.entry(r.author).or_default() += r.len;
    }
    scratch.iter().max_by_key(|&(&a, &n)| (n, std::cmp::Reverse(a))).map(|(&a, _)| a)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splices_insertions_and_deletions() {
        let old = [Run { author: 1, len: 5 }];
        // replace lines 1..3 with 1 new line, append 2 lines at the end
        let hunks = [Hunk { before: 1..3, after: 1..2 }, Hunk { before: 5..5, after: 4..6 }];
        let mut out = Vec::new();
        splice(&old, hunks.into_iter(), 2, &mut out);
        assert_eq!(out, [Run { author: 1, len: 1 }, Run { author: 2, len: 1 }, Run { author: 1, len: 2 }, Run { author: 2, len: 2 }]);
        assert_eq!(dominant(&out, &mut FxHashMap::default()), Some(1));
    }
}
