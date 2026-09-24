use std::io::Read;

use flate2::Decompress;
use rustc_hash::FxHashMap;

use super::pack::{DeltaCache, Pack};
use super::{Kind, Oid};
use crate::{Error, Result};

/// All objects of a repository: packs plus (compressed) loose objects.
pub struct Store {
    packs: Vec<Pack>,
    loose: FxHashMap<Oid, Box<[u8]>>,
    cache: DeltaCache,
    z: Decompress,
}

impl Default for Store {
    fn default() -> Self {
        Store { packs: Vec::new(), loose: FxHashMap::default(), cache: DeltaCache::new(), z: Decompress::new(true) }
    }
}

impl Store {
    pub fn add_pack(&mut self, pack: Pack) {
        self.packs.push(pack);
    }

    pub fn add_loose(&mut self, oid: Oid, compressed: Box<[u8]>) {
        self.loose.insert(oid, compressed);
    }

    pub fn next_pack_id(&self) -> u32 {
        self.packs.len() as u32
    }

    pub fn contains(&self, oid: &Oid) -> bool {
        self.loose.contains_key(oid) || self.packs.iter().any(|p| p.contains(oid))
    }

    pub fn read(&mut self, oid: &Oid, out: &mut Vec<u8>) -> Result<Kind> {
        for pack in &self.packs {
            if let Some(kind) = pack.decode(oid, out, &mut self.cache, &mut self.z)? {
                return Ok(kind);
            }
        }
        let compressed = self.loose.get(oid).ok_or(Error::Missing(*oid))?;
        out.clear();
        flate2::read::ZlibDecoder::new(&compressed[..]).read_to_end(out).map_err(|_| Error::Corrupt("loose object"))?;
        let nul = out.iter().position(|&b| b == 0).ok_or(Error::Corrupt("loose object"))?;
        let kind = out[..nul].split(|&b| b == b' ').next().and_then(Kind::from_name).ok_or(Error::Corrupt("loose object"))?;
        out.drain(..=nul);
        Ok(kind)
    }

    pub fn read_as(&mut self, oid: &Oid, expected: Kind, out: &mut Vec<u8>) -> Result<()> {
        match self.read(oid, out)? {
            kind if kind == expected => Ok(()),
            _ => Err(Error::Corrupt("unexpected object kind")),
        }
    }
}
