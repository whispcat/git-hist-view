use std::fmt;

use sha1::{Digest, Sha1};

use super::object::Kind;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Oid(pub [u8; 20]);

impl Oid {
    pub const LEN: usize = 20;

    pub fn from_slice(bytes: &[u8]) -> Self {
        Oid(bytes.try_into().expect("20-byte oid"))
    }

    pub fn from_hex(hex: &[u8]) -> Option<Self> {
        if hex.len() != 40 {
            return None;
        }
        let nibble = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        let mut out = [0u8; 20];
        for (byte, [hi, lo]) in out.iter_mut().zip(hex.as_chunks::<2>().0) {
            *byte = nibble(*hi)? << 4 | nibble(*lo)?;
        }
        Some(Oid(out))
    }

    pub fn hash_object(kind: Kind, data: &[u8]) -> Self {
        let mut h = Sha1::new();
        h.update(kind.name());
        h.update(b" ");
        h.update(data.len().to_string());
        h.update(b"\0");
        h.update(data);
        Oid(h.finalize().into())
    }
}

impl fmt::Display for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for Oid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self}")
    }
}
