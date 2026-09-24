use std::{fs, io, path::Path};

use ghv_core::git::{Oid, Store, pack::Pack, refs};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// Loads a repository from disk the same way the browser loads a dropped folder.
pub fn open(path: &Path) -> Result<(Store, Oid)> {
    let git = if path.join(".git/HEAD").is_file() { path.join(".git") } else { path.to_path_buf() };
    let mut store = Store::default();

    let pack_dir = git.join("objects/pack");
    for entry in fs::read_dir(&pack_dir).into_iter().flatten() {
        let p = entry?.path();
        if p.extension().is_some_and(|e| e == "pack") {
            let pack = Pack::with_idx(store.next_pack_id(), fs::read(&p)?.into_boxed_slice(), &fs::read(p.with_extension("idx"))?)?;
            store.add_pack(pack);
        }
    }
    for dir in fs::read_dir(git.join("objects"))? {
        let dir = dir?;
        let prefix = dir.file_name().to_string_lossy().into_owned();
        if prefix.len() != 2 || !prefix.bytes().all(|b| b.is_ascii_hexdigit()) {
            continue;
        }
        for obj in fs::read_dir(dir.path())? {
            let obj = obj?;
            let hex = format!("{prefix}{}", obj.file_name().to_string_lossy());
            if let Some(oid) = Oid::from_hex(hex.as_bytes()) {
                store.add_loose(oid, fs::read(obj.path())?.into_boxed_slice());
            }
        }
    }

    let head = match refs::parse_head(&fs::read(git.join("HEAD"))?).ok_or("unreadable HEAD")? {
        refs::HeadRef::Detached(oid) => oid,
        refs::HeadRef::Symbolic(name) => match fs::read(git.join(&name)) {
            Ok(loose) => Oid::from_hex(loose.trim_ascii()).ok_or("bad ref")?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                refs::find_packed(&fs::read(git.join("packed-refs"))?, &name).ok_or_else(|| format!("cannot resolve {name}"))?
            }
            Err(e) => return Err(e.into()),
        },
    };
    Ok((store, head))
}
