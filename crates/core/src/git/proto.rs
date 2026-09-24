//! Sans-IO git protocol v2 over smart HTTP: request builders and a streaming response demuxer.

use super::Oid;
use crate::{Error, Result};

pub const AGENT: &str = "git/2.47-git-hist-view";

fn pkt(out: &mut Vec<u8>, line: &str) {
    out.extend_from_slice(format!("{:04x}", line.len() + 4).as_bytes());
    out.extend_from_slice(line.as_bytes());
}

const FLUSH: &[u8] = b"0000";
const DELIM: &[u8] = b"0001";

enum Pkt<'a> {
    Flush,
    Delim,
    End,
    Data(&'a [u8]),
}

/// Splits the next pkt-line off `buf`, or returns `None` if it is incomplete.
fn next_pkt(buf: &[u8]) -> Result<Option<(Pkt<'_>, usize)>> {
    let Some(len) = buf.get(..4) else { return Ok(None) };
    let len = std::str::from_utf8(len)
        .ok()
        .and_then(|s| usize::from_str_radix(s, 16).ok())
        .ok_or_else(|| Error::Protocol("bad pkt-line length".into()))?;
    Ok(match len {
        0 => Some((Pkt::Flush, 4)),
        1 => Some((Pkt::Delim, 4)),
        2 => Some((Pkt::End, 4)),
        3 => return Err(Error::Protocol("bad pkt-line length".into())),
        n => buf.get(4..n).map(|data| (Pkt::Data(data), n)),
    })
}

fn lines(body: &[u8]) -> Result<Vec<&[u8]>> {
    let (mut out, mut rest) = (Vec::new(), body);
    while let Some((p, used)) = next_pkt(rest)? {
        if let Pkt::Data(d) = p {
            if let Some(msg) = d.strip_prefix(b"ERR ") {
                return Err(Error::Protocol(String::from_utf8_lossy(msg).trim().into()));
            }
            out.push(d.strip_suffix(b"\n").unwrap_or(d));
        }
        rest = &rest[used..];
    }
    Ok(out)
}

/// Server capabilities from the `info/refs` v2 advertisement.
#[derive(Debug, Default)]
pub struct Capabilities {
    pub shallow: bool,
    pub filter: bool,
}

pub fn parse_capabilities(body: &[u8]) -> Result<Capabilities> {
    let lines = lines(body)?;
    if !lines.iter().any(|l| *l == b"version 2") {
        return Err(Error::Unsupported("servers without git protocol v2"));
    }
    let fetch = lines.iter().find_map(|l| l.strip_prefix(b"fetch=")).unwrap_or_default();
    let has = |feature: &[u8]| fetch.split(|&b| b == b' ').any(|f| f == feature);
    Ok(Capabilities { shallow: has(b"shallow"), filter: has(b"filter") })
}

pub fn ls_refs_request() -> Vec<u8> {
    let mut out = Vec::new();
    pkt(&mut out, "command=ls-refs\n");
    pkt(&mut out, &format!("agent={AGENT}\n"));
    out.extend_from_slice(DELIM);
    pkt(&mut out, "symrefs\n");
    pkt(&mut out, "ref-prefix HEAD\n");
    out.extend_from_slice(FLUSH);
    out
}

#[derive(Debug, Clone)]
pub struct Head {
    pub oid: Oid,
    pub branch: Option<String>,
}

pub fn parse_ls_refs(body: &[u8]) -> Result<Head> {
    for line in lines(body)? {
        let mut parts = line.split(|&b| b == b' ');
        let (Some(oid), Some(b"HEAD")) = (parts.next(), parts.next()) else { continue };
        let oid = Oid::from_hex(oid).ok_or_else(|| Error::Protocol("bad oid in ls-refs".into()))?;
        let branch = parts
            .find_map(|p| p.strip_prefix(b"symref-target:"))
            .map(|t| String::from_utf8_lossy(t.strip_prefix(b"refs/heads/").unwrap_or(t)).into_owned());
        return Ok(Head { oid, branch });
    }
    Err(Error::Protocol("repository has no HEAD (empty?)".into()))
}

pub struct FetchOptions {
    pub depth: Option<u32>,
    pub blob_limit: Option<u32>,
}

pub fn fetch_request(want: Oid, caps: &Capabilities, opts: &FetchOptions) -> Vec<u8> {
    let mut out = Vec::new();
    pkt(&mut out, "command=fetch\n");
    pkt(&mut out, &format!("agent={AGENT}\n"));
    out.extend_from_slice(DELIM);
    pkt(&mut out, "ofs-delta\n");
    if let (Some(depth), true) = (opts.depth, caps.shallow) {
        pkt(&mut out, &format!("deepen {depth}\n"));
    }
    if let (Some(limit), true) = (opts.blob_limit, caps.filter) {
        pkt(&mut out, &format!("filter blob:limit={limit}\n"));
    }
    pkt(&mut out, &format!("want {want}\n"));
    pkt(&mut out, "done\n");
    out.extend_from_slice(FLUSH);
    out
}

#[derive(Debug, PartialEq)]
pub enum Event {
    Progress(String),
    Done,
}

#[derive(PartialEq)]
enum Section {
    Preamble,
    ShallowInfo,
    Packfile,
    Done,
}

/// Incrementally demultiplexes a v2 fetch response, appending pack bytes to `pack`.
pub struct FetchDemux {
    buf: Vec<u8>,
    section: Section,
    pub pack: Vec<u8>,
    pub shallow: Vec<Oid>,
}

impl FetchDemux {
    pub fn new(capacity_hint: usize) -> Self {
        FetchDemux { buf: Vec::new(), section: Section::Preamble, pack: Vec::with_capacity(capacity_hint), shallow: Vec::new() }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Result<Vec<Event>> {
        self.buf.extend_from_slice(chunk);
        let mut events = Vec::new();
        let mut at = 0;
        while let Some((p, used)) = next_pkt(&self.buf[at..])? {
            match (p, &self.section) {
                (Pkt::Data(d), Section::Packfile) => match d.split_first() {
                    Some((1, data)) => {
                        self.pack.try_reserve(data.len())?;
                        self.pack.extend_from_slice(data);
                    }
                    Some((2, msg)) => events.push(Event::Progress(String::from_utf8_lossy(msg).trim().into())),
                    Some((3, msg)) => return Err(Error::Protocol(String::from_utf8_lossy(msg).trim().into())),
                    _ => return Err(Error::Protocol("bad sideband channel".into())),
                },
                (Pkt::Data(d), _) => {
                    let line = d.strip_suffix(b"\n").unwrap_or(d);
                    if let Some(msg) = line.strip_prefix(b"ERR ") {
                        return Err(Error::Protocol(String::from_utf8_lossy(msg).into()));
                    }
                    match line {
                        b"packfile" => self.section = Section::Packfile,
                        b"shallow-info" => self.section = Section::ShallowInfo,
                        _ if self.section == Section::ShallowInfo => {
                            if let Some(oid) = line.strip_prefix(b"shallow ").and_then(Oid::from_hex) {
                                self.shallow.push(oid);
                            }
                        }
                        _ => {}
                    }
                }
                (Pkt::Flush | Pkt::End, Section::Packfile) => {
                    self.section = Section::Done;
                    events.push(Event::Done);
                }
                _ => {}
            }
            at += used;
        }
        self.buf.drain(..at);
        Ok(events)
    }

    pub fn is_done(&self) -> bool {
        self.section == Section::Done
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkts(lines: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for l in lines {
            match *l {
                b"0000" | b"0001" => out.extend_from_slice(l),
                _ => {
                    out.extend_from_slice(format!("{:04x}", l.len() + 4).as_bytes());
                    out.extend_from_slice(l);
                }
            }
        }
        out
    }

    #[test]
    fn parses_head_symref() {
        let body = pkts(&[b"1111111111111111111111111111111111111111 HEAD symref-target:refs/heads/main\n", b"0000"]);
        let head = parse_ls_refs(&body).unwrap();
        assert_eq!(head.branch.as_deref(), Some("main"));
    }

    #[test]
    fn demuxes_split_chunks() {
        let body = pkts(&[
            b"shallow-info\n",
            b"shallow 2222222222222222222222222222222222222222\n",
            b"0001",
            b"packfile\n",
            b"\x02Counting objects\n",
            b"\x01PACK",
            b"\x01rest",
            b"0000",
        ]);
        let mut demux = FetchDemux::new(0);
        let mut events = Vec::new();
        for chunk in body.chunks(3) {
            events.extend(demux.push(chunk).unwrap());
        }
        assert_eq!(demux.pack, b"PACKrest");
        assert_eq!(demux.shallow.len(), 1);
        assert_eq!(events, [Event::Progress("Counting objects".into()), Event::Done]);
    }
}
