use flate2::{Decompress, FlushDecompress, Status};
use rustc_hash::FxHashMap;

use super::{Kind, Oid, delta};
use crate::{Error, Result};

const OFS_DELTA: u8 = 6;
const REF_DELTA: u8 = 7;
/// Delta-base markers in the indexer's entry table: a whole object, or a ref-delta resolved by id later.
const NONE: u32 = u32::MAX;
const REF: u32 = u32::MAX - 1;

#[derive(Clone, Copy)]
enum Base {
    None,
    Ofs(u32),
    Ref(Oid),
}

/// A pack entry's object type, inflated size, offset of its compressed data, and delta base.
#[derive(Clone, Copy)]
struct Header {
    kind: u8,
    size: usize,
    data: usize,
    base: Base,
}

fn header(pack: &[u8], offset: usize) -> Result<Header> {
    const CORRUPT: Error = Error::Corrupt("pack entry header");
    let mut pos = offset;
    let mut next = || {
        let b = *pack.get(pos).ok_or(CORRUPT)?;
        pos += 1;
        Ok::<u8, Error>(b)
    };
    let mut byte = next()?;
    let kind = (byte >> 4) & 7;
    let mut size = usize::from(byte & 0x0f);
    let mut shift = 4;
    while byte & 0x80 != 0 {
        byte = next()?;
        size |= usize::from(byte & 0x7f).checked_shl(shift).ok_or(Error::Corrupt("pack entry size"))?;
        shift += 7;
    }
    let base = match kind {
        OFS_DELTA => {
            byte = next()?;
            let mut distance = usize::from(byte & 0x7f);
            while byte & 0x80 != 0 {
                byte = next()?;
                distance = ((distance + 1) << 7) | usize::from(byte & 0x7f);
            }
            Base::Ofs(offset.checked_sub(distance).ok_or(Error::Corrupt("ofs-delta base"))? as u32)
        }
        REF_DELTA => {
            let oid = pack.get(pos..pos + Oid::LEN).ok_or(Error::Corrupt("ref-delta base"))?;
            pos += Oid::LEN;
            Base::Ref(Oid::from_slice(oid))
        }
        1..=4 => Base::None,
        _ => return Err(CORRUPT),
    };
    Ok(Header { kind, size, data: pos, base })
}

/// Inflates exactly `size` bytes into `out`, returning how many compressed bytes were consumed.
fn inflate(z: &mut Decompress, input: &[u8], size: usize, out: &mut Vec<u8>) -> Result<usize> {
    const CORRUPT: Error = Error::Corrupt("zlib stream");
    z.reset(true);
    out.clear();
    out.try_reserve(size)?;
    out.resize(size, 0);
    loop {
        let (before_in, before_out) = (z.total_in(), z.total_out());
        let status =
            z.decompress(&input[before_in as usize..], &mut out[before_out as usize..], FlushDecompress::Finish).map_err(|_| CORRUPT)?;
        if status == Status::StreamEnd {
            break;
        }
        if z.total_in() == before_in && z.total_out() == before_out {
            return Err(CORRUPT);
        }
    }
    if z.total_out() as usize != size {
        return Err(CORRUPT);
    }
    Ok(z.total_in() as usize)
}

/// Direct-mapped cache of delta bases, the same strategy git uses: cheap, and chains mostly hit.
pub struct DeltaCache {
    slots: Vec<Option<(u64, Kind, Vec<u8>)>>,
    bytes: usize,
}

impl DeltaCache {
    const SLOTS: usize = 4096;
    const MAX_ENTRY: usize = 1 << 20;
    const MAX_BYTES: usize = 96 << 20;

    pub fn new() -> Self {
        DeltaCache { slots: vec![None; Self::SLOTS], bytes: 0 }
    }

    fn slot(key: u64) -> usize {
        (key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 52) as usize % Self::SLOTS
    }

    fn get(&self, key: u64) -> Option<(Kind, &[u8])> {
        match &self.slots[Self::slot(key)] {
            Some((k, kind, data)) if *k == key => Some((*kind, data)),
            _ => None,
        }
    }

    fn put(&mut self, key: u64, kind: Kind, data: &[u8]) {
        if data.len() > Self::MAX_ENTRY {
            return;
        }
        if self.bytes + data.len() > Self::MAX_BYTES {
            self.slots.iter_mut().for_each(|s| *s = None);
            self.bytes = 0;
        }
        let slot = &mut self.slots[Self::slot(key)];
        if let Some((_, _, old)) = slot {
            self.bytes -= old.len();
        }
        self.bytes += data.len();
        *slot = Some((key, kind, data.to_vec()));
    }
}

impl Default for DeltaCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Sorted oid -> pack offset table with a first-byte fanout, like a v2 `.idx`.
struct Index {
    fanout: [u32; 256],
    oids: Vec<Oid>,
    offsets: Vec<u32>,
}

impl Index {
    fn new(mut pairs: Vec<(Oid, u32)>) -> Self {
        pairs.sort_unstable_by_key(|p| p.0);
        let mut fanout = [0u32; 256];
        for (oid, _) in &pairs {
            fanout[usize::from(oid.0[0])] += 1;
        }
        for i in 1..256 {
            fanout[i] += fanout[i - 1];
        }
        let (oids, offsets) = pairs.into_iter().unzip();
        Index { fanout, oids, offsets }
    }

    fn find(&self, oid: &Oid) -> Option<u32> {
        let b = usize::from(oid.0[0]);
        let lo = if b == 0 { 0 } else { self.fanout[b - 1] as usize };
        let range = lo..self.fanout[b] as usize;
        let i = self.oids[range.clone()].binary_search(oid).ok()?;
        Some(self.offsets[range.start + i])
    }
}

pub struct Pack {
    id: u32,
    data: Box<[u8]>,
    index: Index,
}

impl Pack {
    fn count(data: &[u8]) -> Result<u32> {
        if data.len() < 32 || &data[..4] != b"PACK" || !matches!(data[7], 2 | 3) {
            return Err(Error::Corrupt("pack header"));
        }
        if u32::try_from(data.len()).is_err() {
            return Err(Error::Unsupported("packs larger than 4 GiB"));
        }
        Ok(u32::from_be_bytes(data[8..12].try_into().unwrap()))
    }

    /// Loads a pack alongside its existing v2 `.idx`, as found in a local repository.
    pub fn with_idx(id: u32, data: Box<[u8]>, idx: &[u8]) -> Result<Self> {
        Self::count(&data)?;
        if idx.get(..8) != Some(&[0xff, b't', b'O', b'c', 0, 0, 0, 2]) {
            return Err(Error::Unsupported("pack index version"));
        }
        let be = |at: usize| idx.get(at..at + 4).map(|b| u32::from_be_bytes(b.try_into().unwrap()));
        let n = be(8 + 255 * 4).ok_or(Error::Corrupt("pack index"))? as usize;
        let oids_at = 8 + 256 * 4;
        let offsets_at = oids_at + n * (Oid::LEN + 4);
        let mut pairs = Vec::new();
        pairs.try_reserve_exact(n)?;
        for i in 0..n {
            let oid = idx.get(oids_at + i * Oid::LEN..oids_at + (i + 1) * Oid::LEN).ok_or(Error::Corrupt("pack index"))?;
            let offset = be(offsets_at + i * 4).ok_or(Error::Corrupt("pack index"))?;
            if offset & 0x8000_0000 != 0 {
                return Err(Error::Unsupported("packs larger than 2 GiB"));
            }
            pairs.push((Oid::from_slice(oid), offset));
        }
        Ok(Pack { id, data, index: Index::new(pairs) })
    }

    /// Builds the oid index for a pack received over the wire by resolving every delta tree once.
    pub fn index(id: u32, data: Box<[u8]>, mut progress: impl FnMut(u32, u32)) -> Result<Self> {
        let total = Self::count(&data)?;
        let n = total as usize;
        let mut z = Decompress::new(true);
        let mut buf = Vec::new();
        let mut offsets = Vec::with_capacity(n);
        let mut bases = Vec::with_capacity(n);
        let mut oids = vec![Oid::default(); n];
        let mut kinds = vec![Kind::Blob; n];
        let mut refs = Vec::new();

        let mut pos = 12;
        for i in 0..n {
            let h = header(&data, pos)?;
            let consumed = inflate(&mut z, &data[h.data..], h.size, &mut buf)?;
            offsets.push(pos as u32);
            bases.push(match h.base {
                Base::None => {
                    kinds[i] = Kind::from_pack_type(h.kind).unwrap();
                    oids[i] = Oid::hash_object(kinds[i], &buf);
                    NONE
                }
                Base::Ofs(off) => offsets.binary_search(&off).map_err(|_| Error::Corrupt("ofs-delta base"))? as u32,
                Base::Ref(oid) => {
                    refs.push((i as u32, oid));
                    REF
                }
            });
            pos = h.data + consumed;
            if i % 4096 == 0 {
                progress(i as u32, total);
            }
        }

        let children = Children::new(&bases);
        let mut resolved = vec![false; n];
        let mut resolver = Resolver {
            data: &data,
            offsets: &offsets,
            children: &children,
            z,
            oids: &mut oids,
            kinds: &mut kinds,
            resolved: &mut resolved,
        };
        for i in 0..n {
            if bases[i] == NONE {
                resolver.resolved[i] = true;
                if !children.of(i).is_empty() {
                    let h = header(&data, offsets[i] as usize)?;
                    let mut content = Vec::new();
                    inflate(&mut resolver.z, &data[h.data..], h.size, &mut content)?;
                    resolver.resolve_children(i, &content)?;
                }
            }
            if i % 4096 == 0 {
                progress(total + i as u32, total * 2);
            }
        }

        // Ref-deltas are rare (we request ofs-delta) but valid; resolve them once their base is known.
        while !refs.is_empty() {
            let known: FxHashMap<Oid, usize> = (0..n).filter(|&i| resolver.resolved[i]).map(|i| (resolver.oids[i], i)).collect();
            let before = refs.len();
            let mut cache = DeltaCache::new();
            let mut pending = Vec::new();
            for (child, base_oid) in refs {
                let Some(&base) = known.get(&base_oid) else {
                    pending.push((child, base_oid));
                    continue;
                };
                let mut content = Vec::new();
                let kind =
                    decode_at(&data, offsets[base], &mut content, &mut cache, id, &mut resolver.z, &|o| known.get(o).map(|&i| offsets[i]))?;
                resolver.resolve_one(child as usize, kind, &content)?;
            }
            refs = pending;
            if refs.len() == before {
                return Err(Error::Missing(refs[0].1));
            }
        }

        let pairs = oids.into_iter().zip(offsets).collect();
        Ok(Pack { id, data, index: Index::new(pairs) })
    }

    pub fn contains(&self, oid: &Oid) -> bool {
        self.index.find(oid).is_some()
    }

    pub fn oids(&self) -> &[Oid] {
        &self.index.oids
    }

    pub fn decode(&self, oid: &Oid, out: &mut Vec<u8>, cache: &mut DeltaCache, z: &mut Decompress) -> Result<Option<Kind>> {
        let Some(offset) = self.index.find(oid) else {
            return Ok(None);
        };
        decode_at(&self.data, offset, out, cache, self.id, z, &|o| self.index.find(o)).map(Some)
    }
}

/// Compressed sparse adjacency: delta children grouped by base entry.
struct Children {
    start: Vec<u32>,
    items: Vec<u32>,
}

impl Children {
    fn new(bases: &[u32]) -> Self {
        let mut start = vec![0u32; bases.len() + 1];
        for &b in bases.iter().filter(|&&b| b < REF) {
            start[b as usize + 1] += 1;
        }
        for i in 1..start.len() {
            start[i] += start[i - 1];
        }
        let mut fill = start.clone();
        let mut items = vec![0u32; start[bases.len()] as usize];
        for (i, &b) in bases.iter().enumerate().filter(|(_, b)| **b < REF) {
            items[fill[b as usize] as usize] = i as u32;
            fill[b as usize] += 1;
        }
        Children { start, items }
    }

    fn of(&self, i: usize) -> &[u32] {
        &self.items[self.start[i] as usize..self.start[i + 1] as usize]
    }
}

struct Resolver<'a> {
    data: &'a [u8],
    offsets: &'a [u32],
    children: &'a Children,
    z: Decompress,
    oids: &'a mut [Oid],
    kinds: &'a mut [Kind],
    resolved: &'a mut [bool],
}

impl Resolver<'_> {
    fn resolve_children(&mut self, base: usize, content: &[u8]) -> Result<()> {
        for &child in self.children.of(base) {
            self.resolve_one(child as usize, self.kinds[base], content)?;
        }
        Ok(())
    }

    fn resolve_one(&mut self, i: usize, kind: Kind, base: &[u8]) -> Result<()> {
        let h = header(self.data, self.offsets[i] as usize)?;
        let mut delta_buf = Vec::new();
        inflate(&mut self.z, &self.data[h.data..], h.size, &mut delta_buf)?;
        let mut content = Vec::new();
        delta::apply(base, &delta_buf, &mut content)?;
        self.kinds[i] = kind;
        self.oids[i] = Oid::hash_object(kind, &content);
        self.resolved[i] = true;
        self.resolve_children(i, &content)
    }
}

fn decode_at(
    data: &[u8],
    offset: u32,
    out: &mut Vec<u8>,
    cache: &mut DeltaCache,
    pack_id: u32,
    z: &mut Decompress,
    find: &dyn Fn(&Oid) -> Option<u32>,
) -> Result<Kind> {
    let key = |off: u32| u64::from(pack_id) << 32 | u64::from(off);
    let mut chain = Vec::new();
    let mut off = offset;
    let kind = loop {
        if let Some((kind, bytes)) = cache.get(key(off)) {
            out.clear();
            out.extend_from_slice(bytes);
            break kind;
        }
        let h = header(data, off as usize)?;
        match h.base {
            Base::None => {
                inflate(z, &data[h.data..], h.size, out)?;
                let kind = Kind::from_pack_type(h.kind).unwrap();
                if !chain.is_empty() {
                    cache.put(key(off), kind, out);
                }
                break kind;
            }
            Base::Ofs(base) => {
                chain.push((off, h));
                off = base;
            }
            Base::Ref(oid) => {
                chain.push((off, h));
                off = find(&oid).ok_or(Error::Missing(oid))?;
            }
        }
    };
    let (mut delta_buf, mut target) = (Vec::new(), Vec::new());
    for (depth, (off, h)) in chain.iter().enumerate().rev() {
        inflate(z, &data[h.data..], h.size, &mut delta_buf)?;
        delta::apply(out, &delta_buf, &mut target)?;
        std::mem::swap(out, &mut target);
        if depth > 0 {
            cache.put(key(*off), kind, out);
        }
    }
    Ok(kind)
}
