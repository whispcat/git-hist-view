use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    time::Instant,
};

use ghv_cli::repo;
use ghv_core::git::{Commit, Kind, Oid, Store, pack::Pack, proto};
use ghv_core::history::{Authors, first_parent};

use crate::CliResult;

/// Indexes a pack from scratch, compares the result with git's `.idx`, then decodes and re-hashes every object.
pub fn verify_pack(path: &Path) -> CliResult {
    let data = fs::read(path)?.into_boxed_slice();
    let mb = data.len() as f64 / 1e6;
    let t = Instant::now();
    let pack = Pack::index(0, data.clone(), |_, _| {})?;
    let secs = t.elapsed().as_secs_f64();
    println!("indexed {} objects, {mb:.1} MB in {secs:.2}s ({:.0} MB/s)", pack.oids().len(), mb / secs);

    let reference = Pack::with_idx(1, data, &fs::read(path.with_extension("idx"))?)?;
    if reference.oids() != pack.oids() {
        return Err("oid set differs from git's .idx".into());
    }

    let mut store = Store::default();
    store.add_pack(pack);
    let (t, mut buf, mut blobs) = (Instant::now(), Vec::new(), 0);
    for oid in reference.oids() {
        let kind = store.read(oid, &mut buf)?;
        blobs += usize::from(kind == Kind::Blob);
        if Oid::hash_object(kind, &buf) != *oid {
            return Err(format!("hash mismatch for {oid}").into());
        }
    }
    println!("decoded and verified {} objects ({blobs} blobs) in {:.2}s", reference.oids().len(), t.elapsed().as_secs_f64());
    Ok(())
}

pub fn walk(path: &Path) -> CliResult {
    let t = Instant::now();
    let (mut store, head) = repo::open(path)?;
    let loaded = t.elapsed().as_secs_f64();
    let mut authors = Authors::default();
    let commits = first_parent(&mut store, head, usize::MAX, &mut authors)?.commits;
    let walked = t.elapsed().as_secs_f64() - loaded;
    println!("{} first-parent commits, {} authors (load {loaded:.2}s, walk {walked:.2}s)", commits.len(), authors.list.len());
    Ok(())
}

/// One smart-HTTP request through curl, keeping the CLI free of an HTTP client dependency.
fn http(url: &str, body: Option<&[u8]>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut cmd = Command::new("curl");
    cmd.args(["-sSf", "--compressed", "-H", "Git-Protocol: version=2", url]);
    if body.is_some() {
        cmd.args(["-H", "Content-Type: application/x-git-upload-pack-request", "--data-binary", "@-"]);
    }
    let mut child = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;
    if let Some(body) = body {
        child.stdin.take().expect("stdin is piped").write_all(body)?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        return Err(format!("request to {url} failed").into());
    }
    Ok(out.stdout)
}

/// Fetches over protocol v2 exactly as the browser does (without the proxy), then walks first-parent history.
pub fn fetch(url: &str, depth: Option<u32>) -> CliResult {
    let url = url.trim_end_matches('/');
    let t = Instant::now();
    let caps = proto::parse_capabilities(&http(&format!("{url}/info/refs?service=git-upload-pack"), None)?)?;
    let head = proto::parse_ls_refs(&http(&format!("{url}/git-upload-pack"), Some(&proto::ls_refs_request()))?)?;
    println!("{caps:?} HEAD {} ({:?})", head.oid, head.branch);

    let opts = proto::FetchOptions { depth, blob_limit: Some(1 << 20) };
    let body = http(&format!("{url}/git-upload-pack"), Some(&proto::fetch_request(head.oid, &caps, &opts)))?;
    let mut demux = proto::FetchDemux::new(0);
    demux.push(&body)?;
    let mb = demux.pack.len() as f64 / 1e6;
    println!("pack {mb:.1} MB, {} shallow, complete: {} in {:.2}s", demux.shallow.len(), demux.is_done(), t.elapsed().as_secs_f64());

    let t = Instant::now();
    let pack = Pack::index(0, demux.pack.into_boxed_slice(), |_, _| {})?;
    println!("indexed {} objects in {:.2}s", pack.oids().len(), t.elapsed().as_secs_f64());

    let mut store = Store::default();
    store.add_pack(pack);
    let (mut next, mut buf, mut commits) = (Some(head.oid), Vec::new(), 0);
    while let Some(oid) = next.filter(|o| store.contains(o)) {
        store.read_as(&oid, Kind::Commit, &mut buf)?;
        next = Commit::parse(&buf)?.parents.first().copied();
        commits += 1;
    }
    println!("first-parent commits: {commits}");
    Ok(())
}
