//! Import extraction with tree-sitter. Each file yields compact text records the resolver understands:
//!
//! - `j <specifier>`: a JS/TS module specifier (import, export-from, require, dynamic import)
//! - `m <name>` / `M <name> <path>`: a Rust `mod name;` declaration, optionally with `#[path = "..."]`
//! - `u <path>`: a flattened Rust `use` path such as `crate::a::b`
//! - `i <module>`: a Python `import a.b`
//! - `f <level> <module|-> <names>`: a Python `from ..module import a,b`

use tree_sitter::{Language, Node, Parser};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    JavaScript,
    TypeScript,
    Tsx,
    Rust,
    Python,
}

impl Lang {
    pub const ALL: [Lang; 5] = [Lang::JavaScript, Lang::TypeScript, Lang::Tsx, Lang::Rust, Lang::Python];

    pub fn from_name(name: &str) -> Option<Lang> {
        let ext = name.rsplit_once('.')?.1;
        Some(match ext {
            "js" | "jsx" | "mjs" | "cjs" => Lang::JavaScript,
            "ts" | "mts" | "cts" => Lang::TypeScript,
            "tsx" => Lang::Tsx,
            "rs" => Lang::Rust,
            "py" | "pyi" => Lang::Python,
            _ => return None,
        })
    }

    fn language(self) -> Language {
        match self {
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }
}

/// Reuses one parser per language across files.
#[derive(Default)]
pub struct Extractor {
    parsers: Vec<(Lang, Parser)>,
}

impl Extractor {
    pub fn extract(&mut self, lang: Lang, src: &[u8]) -> Vec<String> {
        if !self.parsers.iter().any(|(l, _)| *l == lang) {
            let mut parser = Parser::new();
            parser.set_language(&lang.language()).expect("grammar matches tree-sitter version");
            self.parsers.push((lang, parser));
        }
        let parser = &mut self.parsers.iter_mut().find(|(l, _)| *l == lang).unwrap().1;
        let Some(tree) = parser.parse(src, None) else { return Vec::new() };
        let mut out = Vec::new();
        let walk = match lang {
            Lang::Rust => rust,
            Lang::Python => python,
            _ => javascript,
        };
        visit(tree.root_node(), &mut |n| walk(n, src, &mut out));
        out.sort();
        out.dedup();
        out
    }
}

fn visit<'a>(node: Node<'a>, f: &mut impl FnMut(Node<'a>) -> bool) {
    if !f(node) {
        return;
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit(child, f);
    }
}

fn text<'a>(n: Node, src: &'a [u8]) -> &'a str {
    std::str::from_utf8(&src[n.byte_range()]).unwrap_or_default()
}

fn string_value(n: Node, src: &[u8]) -> Option<String> {
    let mut cursor = n.walk();
    let fragment = n.named_children(&mut cursor).find(|c| matches!(c.kind(), "string_fragment" | "string_content"));
    fragment.map(|f| text(f, src).to_string()).or_else(|| (n.named_child_count() == 0).then(String::new))
}

/// Returns whether to descend into the node's children.
fn javascript(n: Node, src: &[u8], out: &mut Vec<String>) -> bool {
    let spec = match n.kind() {
        "import_statement" | "export_statement" => n.child_by_field_name("source"),
        "import_require_clause" => n.child_by_field_name("source"),
        "call_expression" => {
            let callee = n.child_by_field_name("function");
            let is_import = callee.is_some_and(|f| f.kind() == "import" || (f.kind() == "identifier" && text(f, src) == "require"));
            is_import.then(|| n.child_by_field_name("arguments").and_then(|a| a.named_child(0))).flatten().filter(|a| a.kind() == "string")
        }
        _ => None,
    };
    if let Some(s) = spec.and_then(|s| string_value(s, src)).filter(|s| !s.is_empty()) {
        out.push(format!("j {s}"));
    }
    true
}

fn rust(n: Node, src: &[u8], out: &mut Vec<String>) -> bool {
    match n.kind() {
        "mod_item" if n.child_by_field_name("body").is_none() => {
            // Declarations inside inline modules would need that module's path; only top-level ones map to files.
            let top_level = n.parent().is_some_and(|p| p.kind() == "source_file");
            let Some(name) = n.child_by_field_name("name").filter(|_| top_level) else { return false };
            let path = n
                .prev_named_sibling()
                .filter(|s| s.kind() == "attribute_item")
                .and_then(|s| s.named_child(0))
                .filter(|a| a.named_child(0).is_some_and(|id| text(id, src) == "path"))
                .and_then(|a| a.child_by_field_name("value"))
                .and_then(|v| string_value(v, src));
            match path {
                Some(p) => out.push(format!("M {} {p}", text(name, src))),
                None => out.push(format!("m {}", text(name, src))),
            }
            false
        }
        "use_declaration" => {
            if let Some(arg) = n.child_by_field_name("argument") {
                flatten_use(arg, "", src, out);
            }
            false
        }
        _ => true,
    }
}

fn join(prefix: &str, path: &str) -> String {
    let path: String = path.chars().filter(|c| !c.is_whitespace()).collect();
    match (prefix.is_empty(), path.is_empty()) {
        (true, _) => path,
        (_, true) => prefix.to_string(),
        _ => format!("{prefix}::{path}"),
    }
}

fn flatten_use(n: Node, prefix: &str, src: &[u8], out: &mut Vec<String>) {
    match n.kind() {
        "scoped_use_list" => {
            let base = join(prefix, n.child_by_field_name("path").map_or("", |p| text(p, src)));
            if let Some(list) = n.child_by_field_name("list") {
                flatten_use(list, &base, src, out);
            }
        }
        "use_list" => {
            let mut cursor = n.walk();
            for child in n.named_children(&mut cursor) {
                flatten_use(child, prefix, src, out);
            }
        }
        "use_as_clause" => {
            if let Some(path) = n.child_by_field_name("path") {
                flatten_use(path, prefix, src, out);
            }
        }
        "use_wildcard" => out.push(format!("u {}", join(prefix, n.named_child(0).map_or("", |c| text(c, src))))),
        _ => out.push(format!("u {}", join(prefix, text(n, src)))),
    }
}

fn python(n: Node, src: &[u8], out: &mut Vec<String>) -> bool {
    let named = |field: &str| {
        let mut cursor = n.walk();
        n.children_by_field_name(field, &mut cursor)
            .map(|c| match c.kind() {
                "aliased_import" => c.child_by_field_name("name").map_or("", |x| text(x, src)),
                _ => text(c, src),
            })
            .collect::<Vec<_>>()
    };
    match n.kind() {
        "import_statement" => {
            out.extend(named("name").into_iter().map(|m| format!("i {m}")));
            false
        }
        "import_from_statement" => {
            let Some(module) = n.child_by_field_name("module_name") else { return false };
            let (level, name) = match module.kind() {
                "relative_import" => {
                    let mut cursor = module.walk();
                    let dots = module.named_children(&mut cursor).find(|c| c.kind() == "import_prefix").map_or(0, |p| text(p, src).len());
                    let mut cursor = module.walk();
                    let name = module.named_children(&mut cursor).find(|c| c.kind() == "dotted_name").map_or("", |d| text(d, src));
                    (dots, name)
                }
                _ => (0, text(module, src)),
            };
            let mut names = named("name");
            let mut cursor = n.walk();
            if n.named_children(&mut cursor).any(|c| c.kind() == "wildcard_import") {
                names.push("*");
            }
            out.push(format!("f {level} {} {}", if name.is_empty() { "-" } else { name }, names.join(",")));
            false
        }
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn extract(lang: Lang, src: &str) -> Vec<String> {
        Extractor::default().extract(lang, src.as_bytes())
    }

    #[test]
    fn typescript_specifiers() {
        let src = r#"import a, { b } from "./x"; export * from '../y'; const c = require("z"); import("./d"); import e = require("f");
            export { g } from "./g"; import type { T } from "@scope/pkg/sub"; const n = notRequire("no");"#;
        assert_eq!(extract(Lang::TypeScript, src), ["j ../y", "j ./d", "j ./g", "j ./x", "j @scope/pkg/sub", "j f", "j z"]);
    }

    #[test]
    fn rust_mods_and_flattened_uses() {
        let src = "#[path = \"other/p.rs\"] mod a; pub mod b; mod inline { mod nested; use crate::deep; }
            use crate::{x::y, z as w, v::{self, q::*}}; use super::*; use std::io; fn f() { use self::local::Thing; }";
        assert_eq!(
            extract(Lang::Rust, src),
            [
                "M a other/p.rs",
                "m b",
                "u crate::deep",
                "u crate::v::q",
                "u crate::v::self",
                "u crate::x::y",
                "u crate::z",
                "u self::local::Thing",
                "u std::io",
                "u super"
            ]
        );
    }

    #[test]
    fn python_imports() {
        let src = "import a.b, c as d\nfrom . import e\nfrom ..f.g import h, i as j\nfrom k import *\ndef x():\n    import lazy\n";
        assert_eq!(extract(Lang::Python, src), ["f 0 k *", "f 1 - e", "f 2 f.g h,i", "i a.b", "i c", "i lazy"]);
    }
}
