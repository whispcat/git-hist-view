use std::{
    path::{Path, PathBuf},
    process::Command,
};

pub fn git(repo: &Path, args: &[&str]) -> String {
    let out = Command::new("git").args(["-c", "core.quotePath=false", "-C"]).arg(repo).args(args).output().expect("git available");
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8(out.stdout).expect("utf8 git output")
}

/// The synthetic fixture (always built) plus any real clones present in fixtures/repos.
pub fn repos() -> Vec<PathBuf> {
    let fixtures = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let synthetic = fixtures.join("synthetic/basic");
    if !synthetic.exists() {
        let ok = Command::new(fixtures.join("make-repos.sh")).status().expect("run make-repos.sh").success();
        assert!(ok, "make-repos.sh failed");
    }
    let mut repos = vec![synthetic];
    if let Ok(dir) = std::fs::read_dir(fixtures.join("repos")) {
        let mut real: Vec<_> = dir.flatten().map(|e| e.path()).collect();
        real.sort();
        repos.extend(real);
    }
    repos
}
