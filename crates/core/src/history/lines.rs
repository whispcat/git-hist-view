use std::hash::BuildHasher;

use imara_diff::{Algorithm, Diff, Hunk, InternedInput};
use rustc_hash::FxBuildHasher;

/// Git's heuristic: a NUL byte in the first 8000 bytes means binary.
pub fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(8000)].contains(&0)
}

/// Hashes each line including its terminator, so "x" -> "x\n" counts as a change like git does.
pub fn hash_lines(data: &[u8], out: &mut Vec<u32>) {
    out.clear();
    out.extend(data.split_inclusive(|&b| b == b'\n').map(|line| {
        let h = FxBuildHasher.hash_one(line);
        (h ^ (h >> 32)) as u32
    }));
}

#[derive(Default)]
pub struct Differ {
    input: InternedInput<u32>,
    diff: Diff,
}

impl Differ {
    pub fn diff(&mut self, before: &[u32], after: &[u32]) -> impl Iterator<Item = Hunk> + '_ {
        self.input.clear();
        self.input.update_before(before.iter().copied());
        self.input.update_after(after.iter().copied());
        self.diff.compute_with(Algorithm::Histogram, &self.input.before, &self.input.after, self.input.interner.num_tokens());
        self.diff.postprocess_no_heuristic(&self.input);
        self.diff.hunks()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_trailing_newline_change() {
        let (mut a, mut b) = (Vec::new(), Vec::new());
        hash_lines(b"one\ntwo", &mut a);
        hash_lines(b"one\ntwo\n", &mut b);
        let hunks: Vec<Hunk> = Differ::default().diff(&a, &b).collect();
        assert_eq!(hunks, [Hunk { before: 1..2, after: 1..2 }]);
    }
}
