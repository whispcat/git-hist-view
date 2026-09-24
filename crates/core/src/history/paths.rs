use std::hash::BuildHasher;

use hashbrown::HashTable;
use rustc_hash::FxBuildHasher;

pub type PathId = u32;
pub const ROOT: PathId = 0;

#[derive(Debug)]
pub struct Node {
    pub parent: PathId,
    pub name: Box<str>,
    pub is_dir: bool,
}

/// Append-only tree of interned path components; ids are stable for the whole session.
pub struct Paths {
    nodes: Vec<Node>,
    table: HashTable<PathId>,
}

fn key_hash(parent: PathId, name: &str, is_dir: bool) -> u64 {
    FxBuildHasher.hash_one((parent, name, is_dir))
}

impl Default for Paths {
    fn default() -> Self {
        Paths { nodes: vec![Node { parent: ROOT, name: "".into(), is_dir: true }], table: HashTable::new() }
    }
}

impl Paths {
    pub fn child(&mut self, parent: PathId, name: &[u8], is_dir: bool) -> PathId {
        let name = String::from_utf8_lossy(name);
        let hash = key_hash(parent, &name, is_dir);
        let nodes = &mut self.nodes;
        let eq = |&id: &PathId| {
            let n = &nodes[id as usize];
            n.parent == parent && n.is_dir == is_dir && *n.name == *name
        };
        if let Some(&id) = self.table.find(hash, eq) {
            return id;
        }
        let id = nodes.len() as PathId;
        nodes.push(Node { parent, name: name.into(), is_dir });
        self.table.insert_unique(hash, id, |&id| {
            let n = &nodes[id as usize];
            key_hash(n.parent, &n.name, n.is_dir)
        });
        id
    }

    pub fn get(&self, id: PathId) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.len() == 1
    }

    pub fn depth(&self, mut id: PathId) -> u32 {
        let mut depth = 0;
        while id != ROOT {
            id = self.get(id).parent;
            depth += 1;
        }
        depth
    }

    /// How many trailing path components two paths share, e.g. `a/x/f.rs` and `b/x/f.rs` share 2.
    pub fn common_suffix(&self, mut a: PathId, mut b: PathId) -> usize {
        let mut n = 0;
        while a != ROOT && b != ROOT && self.get(a).name == self.get(b).name {
            (a, b, n) = (self.get(a).parent, self.get(b).parent, n + 1);
        }
        n
    }

    pub fn full(&self, id: PathId) -> String {
        let mut parts = Vec::new();
        let mut at = id;
        while at != ROOT {
            parts.push(&*self.get(at).name);
            at = self.get(at).parent;
        }
        parts.reverse();
        parts.join("/")
    }

    pub fn find(&self, path: &str) -> Option<PathId> {
        let mut at = ROOT;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            let is_dir = parts.peek().is_some();
            let hash = key_hash(at, part, is_dir);
            at = *self.table.find(hash, |&id| {
                let n = self.get(id);
                n.parent == at && n.is_dir == is_dir && &*n.name == part
            })?;
        }
        Some(at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interns_and_resolves() {
        let mut p = Paths::default();
        let src = p.child(ROOT, b"src", true);
        let lib = p.child(src, b"lib.rs", false);
        assert_eq!(p.child(src, b"lib.rs", false), lib);
        assert_ne!(p.child(ROOT, b"src", false), src);
        assert_eq!(p.full(lib), "src/lib.rs");
        assert_eq!(p.find("src/lib.rs"), Some(lib));
        assert_eq!(p.depth(lib), 2);
    }
}
