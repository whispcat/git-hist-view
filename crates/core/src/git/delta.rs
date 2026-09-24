use crate::{Error, Result};

const CORRUPT: Error = Error::Corrupt("delta");

fn varint(data: &[u8], pos: &mut usize) -> Result<usize> {
    let (mut value, mut shift) = (0usize, 0);
    loop {
        let byte = *data.get(*pos).ok_or(CORRUPT)?;
        *pos += 1;
        value |= usize::from(byte & 0x7f).checked_shl(shift).ok_or(CORRUPT)?;
        shift += 7;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
}

/// Applies a git delta (copy/insert instruction stream) to `base`, writing the result into `out`.
pub fn apply(base: &[u8], delta: &[u8], out: &mut Vec<u8>) -> Result<()> {
    let mut pos = 0;
    if varint(delta, &mut pos)? != base.len() {
        return Err(CORRUPT);
    }
    let size = varint(delta, &mut pos)?;
    out.clear();
    out.try_reserve(size)?;
    while let Some(&cmd) = delta.get(pos) {
        pos += 1;
        if cmd & 0x80 != 0 {
            let mut field = |mask: u8, shift: u32| -> Result<usize> {
                if cmd & mask == 0 {
                    return Ok(0);
                }
                let byte = *delta.get(pos).ok_or(CORRUPT)?;
                pos += 1;
                Ok(usize::from(byte) << shift)
            };
            let ofs = field(0x01, 0)? | field(0x02, 8)? | field(0x04, 16)? | field(0x08, 24)?;
            let len = match field(0x10, 0)? | field(0x20, 8)? | field(0x40, 16)? {
                0 => 0x10000,
                n => n,
            };
            out.extend_from_slice(base.get(ofs..ofs + len).ok_or(CORRUPT)?);
        } else if cmd != 0 {
            let len = usize::from(cmd);
            out.extend_from_slice(delta.get(pos..pos + len).ok_or(CORRUPT)?);
            pos += len;
        } else {
            return Err(CORRUPT);
        }
    }
    if out.len() == size { Ok(()) } else { Err(CORRUPT) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copies_and_inserts() {
        let base = b"hello world";
        // base len 11, result len 11: copy "hello " (ofs 0, len 6), insert "there"
        let delta = [11, 11, 0x80 | 0x10, 6, 5, b't', b'h', b'e', b'r', b'e'];
        let mut out = Vec::new();
        apply(base, &delta, &mut out).unwrap();
        assert_eq!(out, b"hello there");
    }
}
