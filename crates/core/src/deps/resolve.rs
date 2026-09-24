use std::rc::Rc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::imports::Import;
use super::json::{self, Json};

/// A commit's files as resolution sees them: every live path, and the parsed imports of source files.
pub struct Snapshot<'a> {
    pub files: FxHashSet<&'a str>,
    pub sources: FxHashMap<&'a str, &'a [Import]>,
}

impl<'a> Snapshot<'a> {
    fn source(&self, path: &str) -> Option<&'a str> {
        self.sources.get_key_value(path).map(|(k, _)| *k)
    }
}

pub type Read<'r> = dyn FnMut(&str) -> Option<String> + 'r;

/// File-level dependency edges `(importer, imported)` across all supported languages.
pub fn resolve<'a>(snap: &Snapshot<'a>, read: &mut Read) -> Vec<(&'a str, &'a str)> {
    let mut edges = Vec::new();
    let mut js = Js::new(snap, read);
    for (&file, imports) in &snap.sources {
        for import in imports.iter() {
            if let Import::Js(spec) = import
                && let Some(target) = js.resolve(snap, file, spec, read)
            {
                edges.push((file, target));
            }
        }
    }
    rust(snap, read, &mut edges);
    python(snap, &mut edges);
    edges.retain(|(a, b)| a != b);
    edges.sort_unstable();
    edges.dedup();
    edges
}

pub fn dirname(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

/// Joins a relative path onto a directory, resolving `.` and `..`; `None` if it escapes the repository.
pub fn join(dir: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = if rel.starts_with('/') { Vec::new() } else { dir.split('/').filter(|s| !s.is_empty()).collect() };
    for seg in rel.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    Some(parts.join("/"))
}

const JS_EXTS: &[&str] = &[".ts", ".tsx", ".d.ts", ".js", ".jsx", ".mjs", ".cjs", ".mts", ".cts"];

/// The source file a JS/TS import of `base` lands on: exact, with an extension, `.js` written for a `.ts`
/// source, or a directory's index file.
fn js_file<'a>(snap: &Snapshot<'a>, base: &str) -> Option<&'a str> {
    let base = base.trim_end_matches('/');
    snap.source(base)
        .or_else(|| JS_EXTS.iter().find_map(|e| snap.source(&format!("{base}{e}"))))
        .or_else(|| {
            [(".js", ".ts"), (".js", ".tsx"), (".jsx", ".tsx"), (".mjs", ".mts"), (".cjs", ".cts")]
                .iter()
                .find_map(|(js, ts)| base.strip_suffix(js).and_then(|stem| snap.source(&format!("{stem}{ts}"))))
        })
        .or_else(|| JS_EXTS.iter().find_map(|e| snap.source(&format!("{base}/index{e}"))))
}

#[derive(Clone, Default)]
struct TsConfig {
    base_url: Option<String>,
    paths: Vec<(String, Vec<String>)>,
}

struct Package {
    name: String,
    dir: String,
    entries: Vec<String>,
}

struct Js {
    configs: FxHashMap<String, Option<Rc<TsConfig>>>,
    packages: Vec<Package>,
}

fn load_tsconfig(path: &str, read: &mut Read, depth: u32) -> Option<TsConfig> {
    let json = json::parse(&read(path)?)?;
    let dir = dirname(path);
    let mut cfg = match json.get("extends").and_then(Json::as_str) {
        Some(parent) if parent.starts_with('.') && depth < 5 => {
            let p = join(dir, parent)?;
            let p = if p.ends_with(".json") { p } else { format!("{p}.json") };
            load_tsconfig(&p, read, depth + 1).unwrap_or_default()
        }
        _ => TsConfig::default(),
    };
    if let Some(opts) = json.get("compilerOptions") {
        if let Some(base) = opts.get("baseUrl").and_then(Json::as_str).and_then(|b| join(dir, b)) {
            cfg.base_url = Some(base);
        }
        if let Some(paths) = opts.get("paths") {
            // Since TS 4.1, `paths` without `baseUrl` are relative to the config file.
            let base = cfg.base_url.clone().unwrap_or_else(|| dir.to_string());
            cfg.paths = paths
                .entries()
                .iter()
                .map(|(pattern, targets)| {
                    (pattern.clone(), targets.items().iter().filter_map(Json::as_str).filter_map(|t| join(&base, t)).collect())
                })
                .collect();
        }
    }
    Some(cfg)
}

fn package_entries(json: &Json) -> Vec<String> {
    let mut out: Vec<String> = ["source", "module", "main"].iter().filter_map(|k| json.get(k)?.as_str().map(String::from)).collect();
    let mut exports = json.get("exports");
    if let Some(dot) = exports.and_then(|e| e.get(".")) {
        exports = Some(dot);
    }
    match exports {
        Some(Json::Str(s)) => out.push(s.clone()),
        Some(e) => out.extend(["import", "default", "require", "node"].iter().filter_map(|k| e.get(k)?.as_str().map(String::from))),
        None => {}
    }
    out.extend(["src/index".to_string(), "index".to_string()]);
    out
}

impl Js {
    fn new(snap: &Snapshot, read: &mut Read) -> Js {
        let packages = snap
            .files
            .iter()
            .filter(|f| f.rsplit('/').next() == Some("package.json"))
            .filter_map(|&f| {
                let json = json::parse(&read(f)?)?;
                let name = json.get("name")?.as_str()?.to_string();
                Some(Package { name, dir: dirname(f).to_string(), entries: package_entries(&json) })
            })
            .collect();
        Js { configs: FxHashMap::default(), packages }
    }

    fn config(&mut self, snap: &Snapshot, dir: &str, read: &mut Read) -> Option<Rc<TsConfig>> {
        if let Some(c) = self.configs.get(dir) {
            return c.clone();
        }
        let candidate = if dir.is_empty() { "tsconfig.json".to_string() } else { format!("{dir}/tsconfig.json") };
        let found = if snap.files.contains(candidate.as_str()) {
            load_tsconfig(&candidate, read, 0).map(Rc::new)
        } else if dir.is_empty() {
            None
        } else {
            self.config(snap, dirname(dir), read)
        };
        self.configs.insert(dir.to_string(), found.clone());
        found
    }

    fn resolve<'a>(&mut self, snap: &Snapshot<'a>, from: &str, spec: &str, read: &mut Read) -> Option<&'a str> {
        let spec = spec.split(['?', '#']).next().unwrap_or(spec);
        if spec.starts_with('.') || spec.starts_with('/') {
            return js_file(snap, &join(dirname(from), spec)?);
        }
        let config = self.config(snap, dirname(from), read);
        if let Some(cfg) = &config {
            for (pattern, targets) in &cfg.paths {
                let star = match pattern.split_once('*') {
                    Some((pre, post)) => spec.strip_prefix(pre).and_then(|s| s.strip_suffix(post)),
                    None => (pattern == spec).then_some(""),
                };
                if let Some(star) = star
                    && let Some(hit) = targets.iter().find_map(|t| js_file(snap, &t.replace('*', star)))
                {
                    return Some(hit);
                }
            }
        }
        let segments = if spec.starts_with('@') { 2 } else { 1 };
        let split = spec.match_indices('/').nth(segments - 1).map_or(spec.len(), |(i, _)| i);
        let (name, rest) = (&spec[..split], spec[split..].trim_start_matches('/'));
        if let Some(pkg) = self.packages.iter().find(|p| p.name == name) {
            let found = if rest.is_empty() {
                pkg.entries.iter().find_map(|e| {
                    // Entries usually point at build output that isn't committed; try the source it came from.
                    let e = e.trim_start_matches("./");
                    let src = e.replacen("dist/", "src/", 1).replacen("lib/", "src/", 1);
                    join(&pkg.dir, e).and_then(|p| js_file(snap, &p)).or_else(|| join(&pkg.dir, &src).and_then(|p| js_file(snap, &p)))
                })
            } else {
                join(&pkg.dir, rest)
                    .and_then(|p| js_file(snap, &p))
                    .or_else(|| join(&pkg.dir, &format!("src/{rest}")).and_then(|p| js_file(snap, &p)))
            };
            if found.is_some() {
                return found;
            }
        }
        let base = config?.base_url.clone()?;
        js_file(snap, &join(&base, spec)?)
    }
}

/// Keys and values from a Cargo.toml's `[package]` and `[lib]` tables; enough to find crate roots.
fn cargo_table(text: &str) -> FxHashMap<String, String> {
    let mut out = FxHashMap::default();
    let mut section = String::new();
    for line in text.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_string();
        } else if let Some((key, value)) = line.split_once('=')
            && (section == "package" || section == "lib")
            && let Some(value) = value.trim().strip_prefix('"').and_then(|v| v.split('"').next())
        {
            out.insert(format!("{section}.{}", key.trim()), value.to_string());
        }
    }
    out
}

struct Root {
    /// Name other crates `use` this one by (only libraries can be depended on).
    lib: Option<String>,
    file: String,
}

fn rust<'a>(snap: &Snapshot<'a>, read: &mut Read, edges: &mut Vec<(&'a str, &'a str)>) {
    let mut roots = Vec::new();
    for &manifest in snap.files.iter().filter(|f| f.rsplit('/').next() == Some("Cargo.toml")) {
        let Some(table) = read(manifest).map(|t| cargo_table(&t)) else { continue };
        let dir = dirname(manifest);
        let lib = table.get("lib.path").map_or("src/lib.rs", String::as_str);
        if let Some(file) = join(dir, lib).filter(|f| snap.sources.contains_key(f.as_str())) {
            roots.push(Root { lib: table.get("package.name").map(|n| n.replace('-', "_")), file });
        }
        let bins = join(dir, "src/bin").unwrap_or_default();
        let main = join(dir, "src/main.rs").filter(|f| snap.sources.contains_key(f.as_str()));
        roots.extend(
            main.into_iter()
                .chain(snap.sources.keys().filter(|s| dirname(s) == bins && s.ends_with(".rs")).map(|s| s.to_string()))
                .map(|file| Root { lib: None, file }),
        );
    }
    roots.sort_by(|a, b| a.file.cmp(&b.file));

    // Module trees: each crate root's `mod` declarations mapped to files, per Rust's lookup rules.
    let mut modules: Vec<FxHashMap<Vec<String>, &'a str>> = Vec::new();
    let mut owner: FxHashMap<&'a str, (usize, Vec<String>)> = FxHashMap::default();
    for (r, root) in roots.iter().enumerate() {
        let mut tree = FxHashMap::default();
        let Some(root_file) = snap.source(&root.file) else { continue };
        let mut stack = vec![(root_file, Vec::<String>::new())];
        while let Some((file, path)) = stack.pop() {
            if tree.values().any(|&f| f == file) {
                continue;
            }
            tree.insert(path.clone(), file);
            owner.entry(file).or_insert_with(|| (r, path.clone()));
            let owns_dir = file == root_file || file.ends_with("mod.rs");
            let child_dir = if owns_dir { dirname(file).to_string() } else { file.trim_end_matches(".rs").to_string() };
            for import in snap.sources[file].iter() {
                let Import::Mod { name, path: explicit } = import else { continue };
                let target = match explicit {
                    Some(p) => join(dirname(file), p).and_then(|p| snap.source(&p)),
                    None => [format!("{name}.rs"), format!("{name}/mod.rs")]
                        .iter()
                        .find_map(|c| join(&child_dir, c).and_then(|p| snap.source(&p))),
                };
                if let Some(t) = target {
                    let mut child = path.clone();
                    child.push(name.clone());
                    stack.push((t, child));
                }
            }
        }
        modules.push(tree);
    }

    for (&file, (r, here)) in &owner {
        for import in snap.sources[file].iter() {
            let Import::Use(segs) = import else { continue };
            let Some(first) = segs.first() else { continue };
            let mut i = 1;
            let (root, mut base) = match first.as_str() {
                "crate" => (*r, Vec::new()),
                "self" => (*r, here.clone()),
                "super" => {
                    let mut b = here.clone();
                    i = 0;
                    while segs.get(i).is_some_and(|s| s == "super") && b.pop().is_some() {
                        i += 1;
                    }
                    (*r, b)
                }
                "std" | "core" | "alloc" => continue,
                name => match roots.iter().position(|x| x.lib.as_deref() == Some(name)) {
                    Some(other) => (other, Vec::new()),
                    // 2018-style paths may start at a child module of the current one.
                    None => {
                        i = 0;
                        let mut child = here.clone();
                        child.push(name.to_string());
                        if !modules[*r].contains_key(&child) {
                            continue;
                        }
                        (*r, here.clone())
                    }
                },
            };
            let tree = &modules[root];
            let mut best = tree.get(&base).copied();
            for seg in &segs[i..] {
                if seg == "self" {
                    continue;
                }
                base.push(seg.clone());
                match tree.get(&base) {
                    Some(&f) => best = Some(f),
                    None => break,
                }
            }
            if let Some(target) = best {
                edges.push((file, target));
            }
        }
    }
}

fn python<'a>(snap: &Snapshot<'a>, edges: &mut Vec<(&'a str, &'a str)>) {
    let module = |parts: &[&str]| {
        let base = parts.join("/");
        snap.source(&format!("{base}.py")).or_else(|| snap.source(&format!("{base}/__init__.py")))
    };
    // Absolute imports search the repository root and a `src/` layout, preferring the longest module prefix.
    let absolute = |parts: &[&str]| {
        (1..=parts.len()).rev().find_map(|n| {
            ["", "src"].iter().find_map(|root| {
                let mut full: Vec<&str> = root.split('/').filter(|s| !s.is_empty()).collect();
                full.extend_from_slice(&parts[..n]);
                module(&full)
            })
        })
    };
    for (&file, imports) in &snap.sources {
        if !file.ends_with(".py") && !file.ends_with(".pyi") {
            continue;
        }
        for import in imports.iter() {
            let Import::Py { level, module: m, names } = import else { continue };
            let m: Vec<&str> = m.iter().map(String::as_str).collect();
            let mut targets = Vec::new();
            if *level == 0 {
                for name in names.iter().filter(|n| *n != "*") {
                    let mut full = m.clone();
                    full.push(name);
                    if let Some(t) = ["", "src"].iter().find_map(|root| {
                        let mut p: Vec<&str> = root.split('/').filter(|s| !s.is_empty()).collect();
                        p.extend_from_slice(&full);
                        module(&p)
                    }) {
                        targets.push(t);
                    }
                }
                if targets.is_empty() {
                    targets.extend(absolute(&m));
                }
            } else {
                let mut package: Vec<&str> = dirname(file).split('/').filter(|s| !s.is_empty()).collect();
                if (1..*level).any(|_| package.pop().is_none()) {
                    continue;
                }
                package.extend_from_slice(&m);
                for name in names.iter().filter(|n| *n != "*") {
                    let mut full = package.clone();
                    full.push(name);
                    targets.extend(module(&full));
                }
                if targets.is_empty() {
                    targets.extend(module(&package));
                }
            }
            edges.extend(targets.into_iter().map(|t| (file, t)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::deps::imports::parse_records;

    /// Resolves a tiny in-memory repository; `files` maps path to either import records (sources) or contents.
    fn run(files: &[(&str, &str)]) -> Vec<(String, String)> {
        let is_source = |p: &str| [".ts", ".tsx", ".js", ".rs", ".py"].iter().any(|e| p.ends_with(e));
        let parsed: Vec<(&str, Vec<Import>)> = files.iter().filter(|(p, _)| is_source(p)).map(|(p, r)| (*p, parse_records(r))).collect();
        let snap =
            Snapshot { files: files.iter().map(|(p, _)| *p).collect(), sources: parsed.iter().map(|(p, i)| (*p, i.as_slice())).collect() };
        let contents: FxHashMap<&str, &str> = files.iter().copied().collect();
        let mut read = |p: &str| contents.get(p).map(|c| c.to_string());
        let mut out: Vec<(String, String)> = resolve(&snap, &mut read).into_iter().map(|(a, b)| (a.into(), b.into())).collect();
        out.sort();
        out
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect()
    }

    #[test]
    fn typescript_relative_index_js_to_ts_paths_extends_and_workspaces() {
        let got = run(&[
            ("tsconfig.base.json", r#"{ "compilerOptions": { "baseUrl": ".", "paths": { "@app/*": ["packages/app/src/*"] } } }"#),
            (
                "packages/app/tsconfig.json",
                r#"{ "extends": "../../tsconfig.base", // inherits paths
            }"#,
            ),
            ("packages/app/src/main.ts", "j ./util\nj ./lib/index.js\nj @app/components\nj @scope/core\nj @scope/core/extra\nj react"),
            ("packages/app/src/util.ts", "j ../../core/src/index"),
            ("packages/app/src/lib/index.ts", ""),
            ("packages/app/src/components/index.tsx", ""),
            ("packages/core/package.json", r#"{ "name": "@scope/core", "exports": { ".": { "import": "./dist/index.js" } } }"#),
            ("packages/core/src/index.ts", ""),
            ("packages/core/src/extra.ts", ""),
        ]);
        assert_eq!(
            got,
            pairs(&[
                ("packages/app/src/main.ts", "packages/app/src/components/index.tsx"),
                ("packages/app/src/main.ts", "packages/app/src/lib/index.ts"),
                ("packages/app/src/main.ts", "packages/app/src/util.ts"),
                ("packages/app/src/main.ts", "packages/core/src/extra.ts"),
                ("packages/app/src/main.ts", "packages/core/src/index.ts"),
                ("packages/app/src/util.ts", "packages/core/src/index.ts"),
            ])
        );
    }

    #[test]
    fn rust_module_tree_uses_and_workspace_crates() {
        let got = run(&[
            ("Cargo.toml", "[workspace]\nmembers = [\"crates/*\"]\n"),
            ("crates/core/Cargo.toml", "[package]\nname = \"my-core\"\n"),
            ("crates/core/src/lib.rs", "m git\nm util\nM special odd/place.rs\nu crate::git::pack::Pack"),
            ("crates/core/src/git/mod.rs", "m pack\nu super::util::helper"),
            ("crates/core/src/git/pack.rs", "u self::inner\nu super::super::special"),
            ("crates/core/src/util.rs", "u std::fmt"),
            ("crates/core/src/odd/place.rs", ""),
            ("crates/cli/Cargo.toml", "[package]\nname = \"cli\"\n"),
            ("crates/cli/src/main.rs", "m args\nu my_core::git\nu args::Parsed"),
            ("crates/cli/src/args.rs", ""),
        ]);
        assert_eq!(
            got,
            pairs(&[
                ("crates/cli/src/main.rs", "crates/cli/src/args.rs"),
                ("crates/cli/src/main.rs", "crates/core/src/git/mod.rs"),
                ("crates/core/src/git/mod.rs", "crates/core/src/util.rs"),
                ("crates/core/src/git/pack.rs", "crates/core/src/odd/place.rs"),
                ("crates/core/src/lib.rs", "crates/core/src/git/pack.rs"),
            ])
        );
    }

    #[test]
    fn python_packages_relative_imports_and_src_layout() {
        let got = run(&[
            ("src/pkg/__init__.py", "f 1 - core"),
            ("src/pkg/core.py", "f 1 util helper\nf 2 - other\ni os"),
            ("src/pkg/util.py", ""),
            ("src/other.py", ""),
            ("app.py", "i pkg.core\nf 0 pkg util\nf 0 pkg.util *"),
        ]);
        assert_eq!(
            got,
            pairs(&[
                ("app.py", "src/pkg/core.py"),
                ("app.py", "src/pkg/util.py"),
                ("src/pkg/__init__.py", "src/pkg/core.py"),
                ("src/pkg/core.py", "src/other.py"),
                ("src/pkg/core.py", "src/pkg/util.py"),
            ])
        );
    }
}
