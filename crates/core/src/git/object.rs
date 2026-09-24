use super::Oid;
use crate::{Error, Result};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Commit = 1,
    Tree = 2,
    Blob = 3,
    Tag = 4,
}

impl Kind {
    pub fn from_pack_type(t: u8) -> Option<Self> {
        Some(match t {
            1 => Kind::Commit,
            2 => Kind::Tree,
            3 => Kind::Blob,
            4 => Kind::Tag,
            _ => return None,
        })
    }

    pub fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"commit" => Kind::Commit,
            b"tree" => Kind::Tree,
            b"blob" => Kind::Blob,
            b"tag" => Kind::Tag,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static [u8] {
        match self {
            Kind::Commit => b"commit",
            Kind::Tree => b"tree",
            Kind::Blob => b"blob",
            Kind::Tag => b"tag",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature<'a> {
    pub name: &'a [u8],
    pub email: &'a [u8],
    pub time: i64,
}

impl<'a> Signature<'a> {
    /// Parses `Name <email> 1700000000 +0100`.
    fn parse(line: &'a [u8]) -> Option<Self> {
        let lt = line.iter().position(|&b| b == b'<')?;
        let gt = lt + line[lt..].iter().position(|&b| b == b'>')?;
        let mut rest = line[gt + 1..].split(|&b| b == b' ').filter(|s| !s.is_empty());
        let time = std::str::from_utf8(rest.next()?).ok()?.parse().ok()?;
        Some(Signature { name: line[..lt].trim_ascii_end(), email: &line[lt + 1..gt], time })
    }
}

#[derive(Debug)]
pub struct Commit<'a> {
    pub tree: Oid,
    pub parents: Vec<Oid>,
    pub author: Signature<'a>,
    pub committer_time: i64,
    pub message: &'a [u8],
}

impl<'a> Commit<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        let corrupt = || Error::Corrupt("commit");
        let (mut tree, mut parents, mut author, mut committer_time) = (None, Vec::new(), None, 0);
        let mut rest = data;
        while let Some(nl) = rest.iter().position(|&b| b == b'\n') {
            let line = &rest[..nl];
            rest = &rest[nl + 1..];
            if line.is_empty() {
                break;
            }
            let (key, value) = line.split_at(line.iter().position(|&b| b == b' ').unwrap_or(line.len()));
            let value = value.get(1..).unwrap_or_default();
            match key {
                b"tree" => tree = Oid::from_hex(value),
                b"parent" => parents.push(Oid::from_hex(value).ok_or_else(corrupt)?),
                b"author" => author = Signature::parse(value),
                b"committer" => committer_time = Signature::parse(value).map_or(0, |s| s.time),
                _ => {}
            }
        }
        Ok(Commit { tree: tree.ok_or_else(corrupt)?, parents, author: author.ok_or_else(corrupt)?, committer_time, message: rest })
    }

    pub fn subject(&self) -> &'a [u8] {
        let end = self.message.iter().position(|&b| b == b'\n').unwrap_or(self.message.len());
        self.message[..end].trim_ascii()
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    Tree,
    Blob,
    Link,
    Submodule,
}

#[derive(Clone, Copy, Debug)]
pub struct TreeEntry<'a> {
    pub kind: EntryKind,
    pub name: &'a [u8],
    pub oid: Oid,
}

pub struct TreeIter<'a>(&'a [u8]);

impl<'a> TreeIter<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        TreeIter(data)
    }
}

impl<'a> Iterator for TreeIter<'a> {
    type Item = Result<TreeEntry<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.0.is_empty() {
            return None;
        }
        let entry = parse_tree_entry(self.0);
        self.0 = match entry {
            Some((_, rest)) => rest,
            None => &[],
        };
        Some(entry.map(|(e, _)| e).ok_or(Error::Corrupt("tree")))
    }
}

fn parse_tree_entry(data: &[u8]) -> Option<(TreeEntry<'_>, &[u8])> {
    let sp = data.iter().position(|&b| b == b' ')?;
    let nul = sp + data[sp..].iter().position(|&b| b == 0)?;
    let oid_end = nul + 1 + Oid::LEN;
    // Parse numerically: some old tools wrote zero-padded modes like "040000".
    let mode = data[..sp].iter().try_fold(0u32, |m, &d| (b'0'..=b'7').contains(&d).then(|| m << 3 | u32::from(d - b'0')))?;
    let kind = match mode & 0o170000 {
        0o040000 => EntryKind::Tree,
        0o120000 => EntryKind::Link,
        0o160000 => EntryKind::Submodule,
        _ => EntryKind::Blob,
    };
    let entry = TreeEntry { kind, name: &data[sp + 1..nul], oid: Oid::from_slice(data.get(nul + 1..oid_end)?) };
    Some((entry, &data[oid_end..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commit_with_gpgsig() {
        let raw = b"tree 4b825dc642cb6eb9a060e54bf8d69288fbee4904\n\
parent 0123456789012345678901234567890123456789\n\
author Ada Lovelace <ada@example.com> 1700000000 +0100\n\
committer Bot <bot@example.com> 1700000100 +0000\n\
gpgsig -----BEGIN PGP SIGNATURE-----\n \n abc\n -----END PGP SIGNATURE-----\n\
\n\
Add engine\n\nBody\n";
        let c = Commit::parse(raw).unwrap();
        assert_eq!(c.parents.len(), 1);
        assert_eq!(c.author.name, b"Ada Lovelace");
        assert_eq!(c.author.email, b"ada@example.com");
        assert_eq!(c.author.time, 1_700_000_000);
        assert_eq!(c.committer_time, 1_700_000_100);
        assert_eq!(c.subject(), b"Add engine");
    }
}
