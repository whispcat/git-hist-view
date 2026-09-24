use super::Oid;

pub enum HeadRef {
    Detached(Oid),
    Symbolic(String),
}

pub fn parse_head(head: &[u8]) -> Option<HeadRef> {
    let head = head.trim_ascii();
    match head.strip_prefix(b"ref: ") {
        Some(name) => Some(HeadRef::Symbolic(String::from_utf8_lossy(name).into_owned())),
        None => Oid::from_hex(head).map(HeadRef::Detached),
    }
}

pub fn find_packed(packed_refs: &[u8], name: &str) -> Option<Oid> {
    packed_refs.split(|&b| b == b'\n').find_map(|line| {
        let (oid, rest) = line.split_at_checked(40)?;
        (rest.strip_prefix(b" ")? == name.as_bytes()).then(|| Oid::from_hex(oid)).flatten()
    })
}
