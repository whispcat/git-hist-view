use super::paths::{PathId, Paths, ROOT};

const EXCLUDED_DIRS: &[&str] = &["node_modules", "vendor", "dist", "third_party", ".yarn"];
pub const LOCKFILES: &[&str] = &[
    "Cargo.lock",
    "package-lock.json",
    "yarn.lock",
    "pnpm-lock.yaml",
    "bun.lockb",
    "bun.lock",
    "poetry.lock",
    "Pipfile.lock",
    "uv.lock",
    "Gemfile.lock",
    "composer.lock",
    "go.sum",
    "flake.lock",
    "mix.lock",
    "Podfile.lock",
    "pubspec.lock",
];

/// Caches, per path id, whether generated/vendored content should be left out of every metric.
#[derive(Default)]
pub struct Excludes {
    enabled: bool,
    state: Vec<u8>,
}

impl Excludes {
    pub fn new(enabled: bool) -> Self {
        Excludes { enabled, state: Vec::new() }
    }

    pub fn is_excluded(&mut self, paths: &Paths, id: PathId) -> bool {
        const UNKNOWN: u8 = 0;
        const KEEP: u8 = 1;
        const SKIP: u8 = 2;
        if !self.enabled || id == ROOT {
            return false;
        }
        if self.state.len() < paths.len() {
            self.state.resize(paths.len(), UNKNOWN);
        }
        if self.state[id as usize] == UNKNOWN {
            let node = paths.get(id);
            let name = &*node.name;
            let own = if node.is_dir { EXCLUDED_DIRS.contains(&name) } else { LOCKFILES.contains(&name) || name.contains(".min.") };
            self.state[id as usize] = if own || self.is_excluded(paths, node.parent) { SKIP } else { KEEP };
        }
        self.state[id as usize] == SKIP
    }
}

/// Dependency manifests: a commit touching only these is a version bump, not a design change.
pub const MANIFESTS: &[&str] = &[
    "package.json",
    "Cargo.toml",
    "pyproject.toml",
    "setup.py",
    "setup.cfg",
    "requirements.txt",
    "Pipfile",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Gemfile",
    "composer.json",
    "mix.exs",
    "pubspec.yaml",
    "Package.swift",
    "deno.json",
];

pub const LANGUAGES: &[(&str, &[&str])] = &[
    ("Other", &[]),
    ("Rust", &["rs"]),
    ("TypeScript", &["ts", "tsx", "mts", "cts"]),
    ("JavaScript", &["js", "jsx", "mjs", "cjs"]),
    ("Python", &["py", "pyi"]),
    ("Go", &["go"]),
    ("C/C++", &["c", "h", "cc", "cpp", "hpp", "cxx"]),
    ("Java/Kotlin", &["java", "kt", "kts"]),
    ("Markup", &["html", "vue", "svelte", "astro"]),
    ("Styles", &["css", "scss", "sass", "less"]),
    ("Docs", &["md", "mdx", "rst", "txt", "adoc"]),
    ("Config", &["json", "jsonc", "toml", "yaml", "yml", "xml", "ini", "cfg"]),
    ("Shell", &["sh", "bash", "zsh", "fish", "ps1"]),
    ("Ruby", &["rb"]),
    ("Swift", &["swift"]),
    ("C#", &["cs"]),
];

/// Files the dependency view parses: the languages `ghv-parse` has grammars for.
pub fn is_source(name: &str) -> bool {
    name.rsplit_once('.')
        .is_some_and(|(_, ext)| matches!(ext, "js" | "jsx" | "mjs" | "cjs" | "ts" | "mts" | "cts" | "tsx" | "rs" | "py" | "pyi"))
}

pub fn language(name: &str) -> u8 {
    let Some((_, ext)) = name.rsplit_once('.') else { return 0 };
    let ext = ext.to_ascii_lowercase();
    LANGUAGES.iter().position(|(_, exts)| exts.contains(&ext.as_str())).unwrap_or(0) as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_vendored_and_lockfiles() {
        let mut p = Paths::default();
        let nm = p.child(ROOT, b"node_modules", true);
        let deep = p.child(nm, b"x.js", false);
        let lock = p.child(ROOT, b"Cargo.lock", false);
        let src = p.child(ROOT, b"lib.rs", false);
        let mut ex = Excludes::new(true);
        assert!(ex.is_excluded(&p, deep) && ex.is_excluded(&p, lock) && !ex.is_excluded(&p, src));
        assert_eq!(language("App.TSX"), 2);
    }
}
